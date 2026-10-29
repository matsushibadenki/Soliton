use super::*;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: &'static str,
    pub download_gb: f64,
    pub working_gb: f64,
}
// Published download estimates: Strata/setup.py MODELS. Workspace includes the Metal repack for Unsloth,
// preparation files and 10 GB headroom; optional expert copies can require additional disk space.
pub const MODELS: &[ModelOption] = &[
    ModelOption {
        id: "Q2_0",
        download_gb: 66.4,
        working_gb: 83.4,
    },
    ModelOption {
        id: "IQ3_XXS",
        download_gb: 75.8,
        working_gb: 92.8,
    },
    ModelOption {
        id: "IQ3_S",
        download_gb: 83.6,
        working_gb: 100.6,
    },
    ModelOption {
        id: "UD-Q4_K_XL",
        download_gb: 111.3,
        working_gb: 210.9,
    },
    ModelOption {
        id: "UD-Q5_K_XL",
        download_gb: 158.3,
        working_gb: 279.2,
    },
    ModelOption {
        id: "UD-Q6_K_XL",
        download_gb: 169.2,
        working_gb: 301.0,
    },
];
#[derive(Default)]
pub(crate) struct Preparation {
    pub child: Option<Child>,
    pub status: String,
    pub model: String,
    pub error: Option<String>,
    pub cancel_at: Option<Instant>,
    pub local_reuse: bool,
}
impl Preparation {
    pub fn busy(&self) -> bool {
        self.child.is_some()
    }
    pub fn snapshot(&self) -> Value {
        json!({"status":if self.status.is_empty(){"idle"}else{&self.status},"model":self.model,"error":self.error,"localReuse":self.local_reuse})
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfo {
    pub path: String,
    pub model: String,
    pub backend: String,
    pub engine_exists: bool,
    pub tokenizer_exists: bool,
    pub valid: bool,
    pub error: Option<String>,
}
fn resolve(base: &Path, value: &str) -> PathBuf {
    let p = PathBuf::from(value);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}
pub fn inspect_config(path: &Path, root: &Path) -> ConfigInfo {
    let mut info = ConfigInfo {
        path: path.to_string_lossy().into(),
        model: String::new(),
        backend: String::new(),
        engine_exists: false,
        tokenizer_exists: false,
        valid: false,
        error: None,
    };
    let parsed = (|| -> Result<Value, String> {
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > 1024 * 1024 {
            return Err("Config exceeds 1 MiB".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|e| e.to_string())
    })();
    match parsed {
        Ok(v) => {
            info.model = v["model_name"].as_str().unwrap_or("").into();
            info.backend = v["backend"].as_str().unwrap_or("").into();
            let cwd = v["cwd"]
                .as_str()
                .map(|p| resolve(root, p))
                .unwrap_or(root.to_path_buf());
            info.engine_exists = v["exe"]
                .as_str()
                .is_some_and(|p| !p.is_empty() && resolve(&cwd, p).is_file());
            info.tokenizer_exists = v["tokenizer"]
                .as_str()
                .is_some_and(|p| resolve(&cwd, p).join("vocab.json").is_file());
            info.valid = v.is_object()
                && v["exe"].as_str().is_some_and(|p| !p.is_empty())
                && v["args"].is_array();
            if !info.valid {
                info.error = Some("Expected an engine config with exe and args".into());
            }
        }
        Err(e) => info.error = Some(e),
    }
    info
}
pub(crate) fn configs(root: &Path) -> Vec<ConfigInfo> {
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("strata-"))
                && p.extension().is_some_and(|x| x == "json")
        })
        .collect();
    paths.sort();
    paths.iter().map(|p| inspect_config(p, root)).collect()
}
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut p = path;
        while !p.exists() {
            p = p.parent()?;
        }
        let name = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return None;
        }
        let stat = unsafe { stat.assume_init() };
        Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}
fn config_for(root: &Path, model: &str) -> PathBuf {
    root.join(format!(
        "strata-{}{}.json",
        if model.starts_with("UD-") {
            "unsloth-"
        } else {
            ""
        },
        model.to_lowercase()
    ))
}
impl Studio {
    pub async fn inspect(&self) -> Value {
        let s = self.settings.read().await.clone();
        let root = PathBuf::from(&s.strata_root);
        let probe=tokio::time::timeout(Duration::from_secs(5),Command::new(&s.python).args(["-c","import sys, importlib.util, json; print(json.dumps({'version': sys.version.split()[0], 'supported':sys.version_info >= (3,10), 'dependencies':all(importlib.util.find_spec(m) is not None for m in ['jinja2','regex','numpy','PIL'])}))"]).kill_on_drop(true).output()).await;
        let python = match probe {
            Ok(Ok(o)) if o.status.success() => {
                serde_json::from_slice::<Value>(&o.stdout).unwrap_or(json!({"supported":false}))
            }
            _ => json!({"supported":false,"dependencies":false}),
        };
        json!({"rootExists":root.join("serve/server.py").is_file(),"setupAvailable":root.join("metal/setup_mac.py").is_file(),"supportedHost":cfg!(all(target_os="macos",target_arch="aarch64")),"metalProfile":crate::metal_policy::profile(self.memory_bytes),"python":python,"configs":configs(&root),"selected":if s.config.is_empty(){None}else{Some(inspect_config(&resolve(&root,&s.config),&root))},"models":MODELS,"freeBytes":free_bytes(&root),"recommended":if self.memory_bytes.unwrap_or(0)>=96*1024*1024*1024{"IQ3_S"}else{"Q2_0"}})
    }
    pub async fn prepare(self: &Arc<Self>, model: &str, confirmed: bool) -> Result<(), String> {
        self.prepare_with_capacity(model, confirmed, free_bytes)
            .await
    }
    async fn prepare_with_capacity(
        self: &Arc<Self>,
        model: &str,
        confirmed: bool,
        capacity: impl FnOnce(&Path) -> Option<u64>,
    ) -> Result<(), String> {
        self.prepare_input(model, confirmed, capacity, None).await
    }
    pub(crate) async fn prepare_input(
        self: &Arc<Self>,
        model: &str,
        confirmed: bool,
        capacity: impl FnOnce(&Path) -> Option<u64>,
        local: Option<crate::local_model::LocalInput>,
    ) -> Result<(), String> {
        if !confirmed {
            return Err("Confirm model preparation before starting".into());
        }
        let option = MODELS
            .iter()
            .find(|m| m.id == model)
            .ok_or("Unsupported Metal model")?;
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err("Model preparation currently requires Apple Silicon macOS".into());
        }
        let _lifecycle = self.lifecycle.lock().await;
        {
            let mut p = self.process.lock().await;
            if p.child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
            {
                return Err("Stop the engine before preparing a model".into());
            }
        }
        let mut job = self.preparation.lock().await;
        if job.busy() {
            return Err("Model preparation is already running".into());
        }
        let s = self.settings.read().await.clone();
        if s.engine != crate::engine::Engine::Strata {
            return Err("Select Strata before preparing a Strata model".into());
        }
        if local
            .as_ref()
            .is_some_and(|input| input.settings_key != (s.strata_root.clone(), s.python.clone()))
        {
            return Err("Settings changed; select the model again".into());
        }
        let root = PathBuf::from(&s.strata_root)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !root.join("metal/setup_mac.py").is_file() || !root.join("serve/server.py").is_file() {
            return Err("Select a Strata source directory with Metal setup".into());
        }
        let local_reuse = local.is_some();
        let needed = if local_reuse {
            option.working_gb - option.download_gb
        } else {
            option.working_gb
        };
        let capacity_path = if local.is_some() {
            root.parent().unwrap_or(&root).join("Strata-data")
        } else {
            root.clone()
        };
        if capacity(&capacity_path).is_some_and(|n| (n as f64) < needed * 1e9) {
            return Err(format!(
                "At least {:.1} GB of working space is required",
                needed
            ));
        }
        // Never install into the selected system/shared interpreter. Bootstrap an isolated Strata .venv,
        // then exec the official setup in the same process group; no shell interpolation.
        let default_bootstrap = r#"
import os, sys, venv, subprocess
from pathlib import Path
assert sys.version_info >= (3, 10), 'Python 3.10+ required'
root = Path.cwd()
env = root / '.venv'
py = env / 'bin/python'
assert not env.is_symlink(), 'Strata .venv must be a dedicated directory'
if not py.is_file():
    venv.EnvBuilder(with_pip=True, symlinks=(os.name != "nt"), system_site_packages=(root / '.bundled-metal.json').is_file()).create(env)
prefix = subprocess.check_output([str(py), '-c', "import sys; print(sys.prefix if sys.prefix != sys.base_prefix else '')"], text=True).strip()
assert prefix and Path(prefix).resolve() == env.resolve(), 'Strata .venv is not isolated'
os.execv(str(py), [str(py), '-u', str(root / 'metal/setup_mac.py'), '--yes', '--setup', '--no-start', '--no-browser', '--vision', 'no', '--model', sys.argv[1], '--context', sys.argv[2], '--data-dir', str(root.parent / 'Strata-data')])
"#;
        let mut local_args = Vec::new();
        let bootstrap = if let Some(input) = local {
            let (stage, data) = crate::local_model::stage(&root, &input)?;
            local_args = vec![
                root.to_string_lossy().into_owned(),
                stage.to_string_lossy().into_owned(),
                data.to_string_lossy().into_owned(),
                include_str!("local_setup.py").to_string(),
            ];
            crate::local_model::LOCAL_BOOTSTRAP
        } else {
            default_bootstrap
        };
        let mut cmd = Command::new(&s.python);
        let context = s.metal_memory.context_for(self.memory_bytes).to_string();
        cmd.current_dir(&root)
            .args(["-u", "-c", bootstrap, model, &context])
            .args(local_args)
            .env("PYTHONUNBUFFERED", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let Some(out) = child.stdout.take() {
            pump(out, self.logs.clone());
        }
        if let Some(out) = child.stderr.take() {
            pump(out, self.logs.clone());
        }
        job.child = Some(child);
        job.local_reuse = local_reuse;
        job.status = "running".into();
        job.model = model.into();
        job.error = None;
        job.cancel_at = None;
        drop(job);
        push_log(
            &self.logs,
            format!(
                "[setup] Preparing {model}; local reuse: {local_reuse}; engine remains stopped"
            ),
        )
        .await;
        let studio = self.clone();
        let model = model.to_string();
        tokio::spawn(async move {
            studio.watch_preparation(root, model).await;
        });
        Ok(())
    }
    async fn watch_preparation(self: Arc<Self>, root: PathBuf, model: String) {
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut job = self.preparation.lock().await;
            let cancelling = job.cancel_at.is_some();
            if job
                .cancel_at
                .is_some_and(|t| t.elapsed() > Duration::from_secs(8))
            {
                if let Some(c) = job.child.as_mut() {
                    kill_group(c, Signal::Kill);
                    let _ = c.start_kill();
                }
            }
            let exit = match job.child.as_mut() {
                Some(c) => c.try_wait(),
                None => return,
            };
            match exit {
                Ok(None) => continue,
                result => {
                    job.child.take();
                    job.cancel_at = None;
                    let success = matches!(result,Ok(Some(status)) if status.success());
                    if cancelling {
                        job.status = "cancelled".into();
                    } else if success {
                        let config = config_for(&root, &model);
                        let memory_settings = self.settings.read().await.metal_memory.clone();
                        let tuning = crate::metal_policy::apply_with_settings(
                            &config,
                            self.memory_bytes,
                            &memory_settings,
                        );
                        let info = inspect_config(&config, &root);
                        if let Err(e) = tuning {
                            job.status = "failed".into();
                            job.error = Some(e);
                        } else if info.valid && info.engine_exists && info.tokenizer_exists {
                            let mut s = self.settings.write().await;
                            s.config = info.path;
                            s.python = root.join(".venv/bin/python").to_string_lossy().into();
                            s.mock = false;
                            job.status = "completed".into();
                        } else {
                            job.status = "failed".into();
                            job.error = Some(
                                "Setup exited without a usable engine config; see logs".into(),
                            );
                        }
                    } else {
                        job.status = "failed".into();
                        job.error = Some(match result {
                            Ok(Some(status)) => format!("Setup exited: {status}"),
                            Err(e) => e.to_string(),
                            _ => "Setup failed".into(),
                        });
                    }
                    let status = job.status.clone();
                    drop(job);
                    push_log(&self.logs, format!("[setup] {status}")).await;
                    if let Err(e) = self.persist().await {
                        push_log(&self.logs, format!("[setup] Save failed: {e}")).await;
                    }
                    return;
                }
            }
        }
    }
    pub async fn cancel_preparation(&self) -> Result<(), String> {
        let mut job = self.preparation.lock().await;
        if job.cancel_at.is_none() {
            if let Some(c) = job.child.as_mut() {
                kill_group(c, Signal::Interrupt);
                job.cancel_at = Some(Instant::now());
                job.status = "cancelling".into();
            }
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), String> {
        self.stop().await?;
        let mut job = self.preparation.lock().await;
        if let Some(mut c) = job.child.take() {
            kill_group(&mut c, Signal::Terminate);
            if tokio::time::timeout(Duration::from_secs(8), c.wait())
                .await
                .is_err()
            {
                kill_group(&mut c, Signal::Kill);
                let _ = c.kill().await;
            }
        }
        Ok(())
    }
}
enum Signal {
    Interrupt,
    Terminate,
    Kill,
}
fn kill_group(child: &mut Child, signal: Signal) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        let code = match signal {
            Signal::Interrupt => libc::SIGINT,
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        unsafe {
            libc::kill(-(id as i32), code);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = signal;
        let _ = child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("strata-prepare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn configuration_inspection_distinguishes_invalid_and_missing_assets() {
        let root = temp_root();
        let path = root.join("strata-test.json");
        std::fs::write(&path, "{oops").unwrap();
        assert!(!inspect_config(&path, &root).valid);
        std::fs::write(
            &path,
            r#"{"exe":"build-metal/strata","args":[],"tokenizer":"pack/tokenizer"}"#,
        )
        .unwrap();
        let info = inspect_config(&path, &root);
        assert!(info.valid);
        assert!(!info.engine_exists);
        assert!(!info.tokenizer_exists);
        std::fs::create_dir_all(root.join("build-metal")).unwrap();
        std::fs::write(root.join("build-metal/strata"), "").unwrap();
        std::fs::create_dir_all(root.join("pack/tokenizer")).unwrap();
        std::fs::write(root.join("pack/tokenizer/vocab.json"), "{}").unwrap();
        let info = inspect_config(&path, &root);
        assert!(info.engine_exists && info.tokenizer_exists);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn download_requires_explicit_consent() {
        let s = Arc::new(Studio::new(PathBuf::new(), 8080));
        assert!(s
            .prepare("Q2_0", false)
            .await
            .unwrap_err()
            .contains("Confirm"));
        assert!(s
            .prepare("shell injection", true)
            .await
            .unwrap_err()
            .contains("Unsupported"));
        assert!(!s.preparation.lock().await.busy());
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn fixture(script: &str) -> (PathBuf, Arc<Studio>) {
        let root = temp_root();
        std::fs::create_dir_all(root.join("metal")).unwrap();
        std::fs::create_dir_all(root.join("serve")).unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        std::fs::write(root.join("serve/server.py"), "").unwrap();
        std::fs::write(root.join("metal/setup_mac.py"), script).unwrap();
        let interpreter = std::env::var("STUDIO_TEST_PYTHON").unwrap_or("python3".into());
        let output = std::process::Command::new(interpreter)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        let python = String::from_utf8(output.stdout).unwrap().trim().to_string();
        assert!(std::process::Command::new(&python)
            .args(["-m", "venv", "--without-pip"])
            .arg(root.join(".venv"))
            .status()
            .unwrap()
            .success());
        let s = Arc::new(Studio::new(root.clone(), 8080));
        (root, s)
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    async fn finished(s: &Studio) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while s.preparation.lock().await.busy() {
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        })
        .await
        .unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn successful_setup_applies_config_without_starting_engine() {
        let (root,s)=fixture("import sys,json\nfrom pathlib import Path\nr=Path.cwd()\nassert '--no-start' in sys.argv and '--no-browser' in sys.argv and '--yes' in sys.argv\n(r/'tokenizer').mkdir()\n(r/'tokenizer/vocab.json').write_text('{}')\n(r/'engine').write_text('fixture')\n(r/'strata-q2_0.json').write_text(json.dumps({'exe':str(r/'engine'),'args':[],'tokenizer':str(r/'tokenizer'),'backend':'metal','model_name':'fixture'}))\nprint('fixture setup complete',flush=True)");
        assert!(s
            .prepare_with_capacity("Q2_0", true, |_| Some(0))
            .await
            .unwrap_err()
            .contains("space"));
        s.prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
            .await
            .unwrap();
        finished(&s).await;
        assert_eq!(s.preparation.lock().await.status, "completed");
        assert!(s.settings.read().await.config.ends_with("strata-q2_0.json"));
        assert!(s.process.lock().await.child.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn preparation_and_repreparation_apply_memory_policy() {
        let (root, s) = fixture("import sys,json\nfrom pathlib import Path\nr=Path.cwd()\n(r/'context-argument').write_text(sys.argv[sys.argv.index('--context')+1])\n(r/'tokenizer').mkdir(exist_ok=True)\n(r/'tokenizer/vocab.json').write_text('{}')\n(r/'engine').write_text('fixture')\n(r/'strata-q2_0.json').write_text(json.dumps({'exe':str(r/'engine'),'args':['--max-context','65536'],'tokenizer':str(r/'tokenizer'),'backend':'metal','env':{'STRATA_METAL_STREAM':'1'}}))\n");
        for memory_settings in [
            crate::metal_policy::MemorySettings::default(),
            crate::metal_policy::MemorySettings {
                automatic: false,
                context: 4096,
                prompt_chunk: 128,
                keep_free_gb: 8,
            },
        ] {
            s.settings.write().await.metal_memory = memory_settings.clone();
            let context = memory_settings.context_for(s.memory_bytes);
            let chunk = if memory_settings.automatic {
                crate::metal_policy::profile(s.memory_bytes).prompt_chunk
            } else {
                memory_settings.prompt_chunk
            };
            for _ in 0..2 {
                s.prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
                    .await
                    .unwrap();
                finished(&s).await;
                assert_eq!(s.preparation.lock().await.status, "completed");
                assert_eq!(
                    std::fs::read_to_string(root.join("context-argument")).unwrap(),
                    context.to_string()
                );
                let config: Value =
                    serde_json::from_slice(&std::fs::read(root.join("strata-q2_0.json")).unwrap())
                        .unwrap();
                assert_eq!(
                    config["args"],
                    json!(["--max-context", context.to_string()])
                );
                assert_eq!(
                    config["env"]["STRATA_METAL_PROMPT_CHUNK"],
                    chunk.to_string()
                );
                assert_eq!(config["env"]["STRATA_METAL_STREAM"], "1");
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn cancellation_and_failure_preserve_existing_engine_config() {
        let (root, s) = fixture("import time\ntime.sleep(60)");
        s.settings.write().await.config = "existing-config.json".into();
        s.prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
            .await
            .unwrap();
        assert!(s.start().await.unwrap_err().contains("preparation"));
        assert!(s
            .prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
            .await
            .is_err());
        s.cancel_preparation().await.unwrap();
        finished(&s).await;
        assert_eq!(s.preparation.lock().await.status, "cancelled");
        assert_eq!(s.settings.read().await.config, "existing-config.json");
        std::fs::write(root.join("metal/setup_mac.py"), "raise SystemExit(7)").unwrap();
        s.prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
            .await
            .unwrap();
        finished(&s).await;
        assert_eq!(s.preparation.lock().await.status, "failed");
        assert!(s.preparation.lock().await.error.is_some());
        assert_eq!(s.settings.read().await.config, "existing-config.json");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn shared_environment_is_never_used_for_setup_installation() {
        let (root, s) = fixture("from pathlib import Path; Path('must-not-run').touch()");
        let shared = root.join("shared-env");
        std::fs::rename(root.join(".venv"), &shared).unwrap();
        // Pointing the source tree at another environment must not let setup install into it.
        std::os::unix::fs::symlink(&shared, root.join(".venv")).unwrap();
        s.prepare_with_capacity("Q2_0", true, |_| Some(u64::MAX))
            .await
            .unwrap();
        finished(&s).await;
        assert_eq!(s.preparation.lock().await.status, "failed");
        assert!(!root.join("must-not-run").exists());
        std::fs::remove_file(root.join(".venv")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
