use super::*;
use std::{
    hash::{Hash, Hasher},
    path::Path,
};
#[derive(Clone, Debug)]
pub(crate) struct LocalInput {
    pub model: String,
    pub files: Vec<PathBuf>,
    pub names: Vec<String>,
    pub settings_key: (String, String),
}
pub(crate) const LOCAL_BOOTSTRAP: &str = r#"
import os, sys, venv, subprocess
from pathlib import Path
root=Path(sys.argv[3]); stage=Path(sys.argv[4]); data=Path(sys.argv[5]); wrapper=sys.argv[6]
env=root/'.venv'; py=env/'bin/python'
assert not env.is_symlink(), 'Strata .venv must be dedicated'
if not py.is_file(): venv.EnvBuilder(with_pip=True, symlinks=(os.name != "nt")).create(env)
prefix=subprocess.check_output([str(py),'-c',"import sys; print(sys.prefix if sys.prefix != sys.base_prefix else '')"],text=True).strip()
assert prefix and Path(prefix).resolve()==env.resolve(), 'Strata .venv is not isolated'
os.execv(str(py),[str(py),'-u','-c',wrapper,str(root),str(stage),str(data),sys.argv[1],sys.argv[2]])
"#;
fn candidate(path: &Path) -> Result<LocalInput, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid model path")?;
    for model in preparation::MODELS {
        let n = match model.id {
            "UD-Q4_K_XL" => 4,
            "UD-Q5_K_XL" | "UD-Q6_K_XL" => 6,
            _ => 2,
        };
        let prefix = if model.id.starts_with("UD-") {
            format!("Qwen3.8-Flash-Next-{}", model.id)
        } else {
            format!("Qwen3.8-Flash-Next-GSQ-RCO-{}", model.id)
        };
        let names: Vec<_> = (1..=n)
            .map(|i| format!("{prefix}-{i:05}-of-{n:05}.gguf"))
            .collect();
        if !names.iter().any(|n| n == name) {
            continue;
        }
        let parent = path.parent().ok_or("Invalid model folder")?;
        let files: Vec<_> = names.iter().map(|n| parent.join(n)).collect();
        let missing: Vec<_> = files
            .iter()
            .filter(|p| !p.is_file())
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        if !missing.is_empty() {
            return Err(format!("Missing model shards: {}", missing.join(", ")));
        }
        return Ok(LocalInput {
            model: model.id.into(),
            files,
            names,
            settings_key: Default::default(),
        });
    }
    Err("This file is not one of the six supported Metal model shard sets".into())
}
async fn validated(settings: &Settings, path: &str) -> Result<LocalInput, String> {
    let mut input = candidate(Path::new(path))?;
    input.settings_key = (settings.strata_root.clone(), settings.python.clone());
    let files = serde_json::to_string(&input.files).map_err(|e| e.to_string())?;
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new(&settings.python)
            .args([
                "-c",
                include_str!("local_validate.py"),
                &settings.strata_root,
                &files,
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Model validation timed out")?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err
            .lines()
            .last()
            .unwrap_or("GGUF validation failed")
            .chars()
            .take(1500)
            .collect());
    }
    Ok(input)
}
fn ready_config(settings: &Settings, path: &str) -> Option<String> {
    let selected = Path::new(path).canonicalize().ok()?;
    let root = PathBuf::from(&settings.strata_root);
    let mut configs = preparation::configs(&root);
    if !settings.config.is_empty() {
        configs.push(preparation::inspect_config(
            Path::new(&settings.config),
            &root,
        ));
    }
    for info in configs {
        if !info.valid || !info.engine_exists || !info.tokenizer_exists {
            continue;
        }
        let Ok(data) = std::fs::read(&info.path) else {
            continue;
        };
        let Ok(v) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        let cwd = v["cwd"]
            .as_str()
            .map(PathBuf::from)
            .map(|p| if p.is_absolute() { p } else { root.join(p) })
            .unwrap_or(root.clone());
        let Some(args) = v["args"].as_array() else {
            continue;
        };
        for pair in args.windows(2) {
            if !["--native", "--ple-gguf", "--native-dense-gguf", "--pack"]
                .contains(&pair[0].as_str().unwrap_or(""))
            {
                continue;
            }
            if let Some(p) = pair[1].as_str() {
                let p = PathBuf::from(p);
                let p = if p.is_absolute() { p } else { cwd.join(p) };
                if let Ok(p) = p.canonicalize() {
                    let pack_file = Path::new(path)
                        .file_name()
                        .is_some_and(|n| n == "experts.bin")
                        && Path::new(path)
                            .parent()
                            .and_then(|parent| parent.canonicalize().ok())
                            .as_ref()
                            == Some(&p);
                    if selected == p || p.is_dir() && pack_file {
                        return Some(info.path);
                    }
                }
            }
        }
    }
    None
}
impl Studio {
    pub async fn inspect_local(&self, path: &str) -> Result<Value, String> {
        let s = self.settings.read().await.clone();
        if let Some(config) = ready_config(&s, path) {
            return Ok(json!({"kind":"configured","config":config,"path":path}));
        }
        let input = match validated(&s, path).await {
            Ok(input) => input,
            Err(reason) => return Ok(json!({"kind":"unsupported", "reason":reason, "path":path})),
        };
        let option = preparation::MODELS
            .iter()
            .find(|m| m.id == input.model)
            .unwrap();
        Ok(
            json!({"kind":"prepare","model":input.model,"path":path,"files":input.files,"workingGb":option.working_gb-option.download_gb,"downloadGb":0,"freeBytes":preparation::free_bytes(&Path::new(&s.strata_root).parent().unwrap_or(Path::new(&s.strata_root)).join("Strata-data"))}),
        )
    }
    pub async fn use_local(self: &Arc<Self>, path: &str, confirmed: bool) -> Result<(), String> {
        if !confirmed {
            return Err("Confirm reuse before preparing a local model".into());
        }
        let s = self.settings.read().await.clone();
        if let Some(config) = ready_config(&s, path) {
            let _guard = self.lifecycle.lock().await;
            if self.preparation.lock().await.busy() || self.process.lock().await.child.is_some() {
                return Err("Stop the engine before switching models".into());
            }
            let mut current = self.settings.write().await;
            if current.engine != crate::engine::Engine::Strata {
                return Err("Select Strata before using a Strata model".into());
            }
            if current.strata_root != s.strata_root || current.config != s.config {
                return Err("Settings changed; select the model again".into());
            }
            let python = Path::new(&current.strata_root).join(".venv/bin/python");
            if python.is_file() {
                current.python = python.to_string_lossy().into_owned();
            }
            current.config = config;
            current.mock = false;
            return Ok(());
        }
        let input = validated(&s, path).await?;
        self.prepare_input(
            &input.model.clone(),
            true,
            preparation::free_bytes,
            Some(input),
        )
        .await
    }
}
pub(crate) fn stage(root: &Path, input: &LocalInput) -> Result<(PathBuf, PathBuf), String> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for file in &input.files {
        let canonical = file.canonicalize().map_err(|e| e.to_string())?;
        canonical.hash(&mut hash);
        let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
        meta.len().hash(&mut hash);
        meta.modified().ok().hash(&mut hash);
    }
    let base = root
        .parent()
        .ok_or("Invalid Strata root")?
        .join("Strata-data/local-models")
        .join(format!("{}-{:016x}", input.model, hash.finish()));
    let stage = base.join("sources");
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    for (file, name) in input.files.iter().zip(&input.names) {
        let dest = stage.join(name);
        let target = file.canonicalize().map_err(|e| e.to_string())?;
        if std::fs::symlink_metadata(&dest).is_ok() {
            if dest.canonicalize().ok().as_ref() != Some(&target) {
                return Err("Local model link points to a different file".into());
            }
            continue;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &dest).map_err(|e| e.to_string())?;
        #[cfg(not(unix))]
        return Err("Local Metal model reuse requires macOS".into());
    }
    Ok((stage, base.join("prepared")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> (PathBuf, Arc<Studio>, PathBuf) {
        let base = std::env::temp_dir().join(format!("strata-reuse-{}", uuid::Uuid::new_v4()));
        let root = base.join("Strata");
        let external = base.join("external");
        for p in [
            root.join("tools"),
            root.join("metal"),
            root.join("serve"),
            external.clone(),
        ] {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(root.join("serve/server.py"), "").unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Strata/tools/gguf_reader.py"),
            root.join("tools/gguf_reader.py"),
        )
        .unwrap();
        let s = Arc::new(Studio::new(root.clone(), 8080));
        (base, s, external)
    }
    fn gguf(path: &Path, index: u32, arch: &str) {
        fn string(b: &mut Vec<u8>, s: &str) {
            b.extend_from_slice(&(s.len() as u64).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
        let mut b = b"GGUF".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&1u64.to_le_bytes());
        b.extend_from_slice(&4u64.to_le_bytes());
        string(&mut b, "general.architecture");
        b.extend_from_slice(&8u32.to_le_bytes());
        string(&mut b, arch);
        for (key, value) in [
            ("split.count", 2),
            ("split.no", index),
            ("split.tensors.count", 2),
        ] {
            string(&mut b, key);
            b.extend_from_slice(&4u32.to_le_bytes());
            b.extend_from_slice(&value.to_le_bytes());
        }
        string(&mut b, &format!("tensor_{index}"));
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&1u64.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.resize((b.len() + 31) / 32 * 32 + 4, 0);
        fs::write(path, b).unwrap();
    }
    fn shards(external: &Path) -> Vec<PathBuf> {
        (1..=2)
            .map(|i| {
                external.join(format!(
                    "Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-{i:05}-of-00002.gguf"
                ))
            })
            .collect()
    }
    #[tokio::test]
    async fn refuses_missing_mixed_and_truncated_shards_before_preparation() {
        let (base, s, external) = fixture();
        let files = shards(&external);
        gguf(&files[0], 0, "qwen4exp");
        assert!(
            validated(&*s.settings.read().await, files[0].to_str().unwrap())
                .await
                .unwrap_err()
                .contains("Missing")
        );
        gguf(&files[1], 0, "qwen4exp");
        assert!(
            validated(&*s.settings.read().await, files[0].to_str().unwrap())
                .await
                .unwrap_err()
                .contains("Inconsistent")
        );
        gguf(&files[1], 1, "llama");
        assert!(
            validated(&*s.settings.read().await, files[0].to_str().unwrap())
                .await
                .unwrap_err()
                .contains("Mixed")
        );
        gguf(&files[1], 1, "qwen4exp");
        let len = fs::metadata(&files[1]).unwrap().len();
        fs::OpenOptions::new()
            .write(true)
            .open(&files[1])
            .unwrap()
            .set_len(len - 2)
            .unwrap();
        assert!(
            validated(&*s.settings.read().await, files[0].to_str().unwrap())
                .await
                .unwrap_err()
                .contains("truncated")
        );
        assert!(!s.preparation.lock().await.busy());
        assert!(!base.join("Strata-data").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stages_links_without_copying_or_touching_source_sidecars() {
        let (base, s, external) = fixture();
        let files = shards(&external);
        for (i, p) in files.iter().enumerate() {
            gguf(p, i as u32, "qwen4exp");
        }
        let input = validated(&*s.settings.read().await, files[1].to_str().unwrap())
            .await
            .unwrap();
        let original = fs::read(&files[0]).unwrap();
        let (stage, data) = stage(&s.root, &input).unwrap();
        assert!(stage.join(&input.names[0]).is_symlink());
        assert_eq!(
            stage.join(&input.names[0]).canonicalize().unwrap(),
            files[0].canonicalize().unwrap()
        );
        assert_eq!(original, fs::read(&files[0]).unwrap());
        assert_eq!(fs::read_dir(&external).unwrap().count(), 2);
        assert!(!data.exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[tokio::test]
    async fn activates_ready_config_without_setup_or_download() {
        let (base, s, external) = fixture();
        let files = shards(&external);
        gguf(&files[0], 0, "qwen4exp");
        fs::create_dir_all(s.root.join("tokenizer")).unwrap();
        fs::write(s.root.join("tokenizer/vocab.json"), "{}").unwrap();
        fs::write(s.root.join("engine"), "fixture").unwrap();
        let config = s.root.join("strata-existing.json");
        fs::write(&config,json!({"exe":s.root.join("engine"),"args":["--pack",external,"--native",files[0]],"tokenizer":s.root.join("tokenizer"),"backend":"metal"}).to_string()).unwrap();
        let unrelated = external.join("unrelated.gguf");
        gguf(&unrelated, 0, "llama");
        assert_eq!(
            s.inspect_local(unrelated.to_str().unwrap()).await.unwrap()["kind"],
            "unsupported"
        );
        assert_eq!(
            s.inspect_local(files[0].to_str().unwrap()).await.unwrap()["kind"],
            "configured"
        );
        assert!(s
            .use_local(files[0].to_str().unwrap(), false)
            .await
            .is_err());
        s.use_local(files[0].to_str().unwrap(), true).await.unwrap();
        assert_eq!(s.settings.read().await.config, config.to_str().unwrap());
        assert!(!base.join("Strata-data").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn local_setup_blocks_downloads_and_applies_generated_config() {
        let (base, s, external) = fixture();
        let files = shards(&external);
        for (i, p) in files.iter().enumerate() {
            gguf(p, i as u32, "qwen4exp");
        }
        let root = &s.root;
        assert!(std::process::Command::new("python3")
            .args(["-m", "venv", "--without-pip"])
            .arg(root.join(".venv"))
            .status()
            .unwrap()
            .success());
        fs::write(root.join("setup.py"),"from pathlib import Path\ndef done(p): return False\ndef data_folder_for(a): return data_folder(None,True,True,True)\ndef download(*a,**k): raise AssertionError('network must never run')\n").unwrap();
        fs::write(root.join("metal/setup_mac.py"),r#"
from pathlib import Path
import json, setup as S
from types import SimpleNamespace
_fit=SimpleNamespace(stream_mtp=lambda *a: True)
ROOT=Path(__file__).resolve().parent.parent
def stream_mtp(*a): return True
def main(args):
    stage=Path(args[args.index('--gguf-dir')+1])
    files=list(stage.glob('*.gguf'))
    assert len(files)==2 and all(p.is_symlink() and S.done(p) for p in files)
    assert not stream_mtp(False,128)
    assert not _fit.stream_mtp(False,128)
    data,others=S.data_folder(None,True,True,True)
    assert S.data_folder_for(object()) == (data, [])
    assert not others
    try: S.download('https://invalid.example/model.gguf',stage/'missing.gguf')
    except RuntimeError: pass
    else: raise AssertionError('model download not blocked')
    (ROOT/'tokenizer').mkdir();(ROOT/'tokenizer/vocab.json').write_text('{}');(ROOT/'engine').write_text('fixture')
    (ROOT/'strata-q2_0.json').write_text(json.dumps({'exe':str(ROOT/'engine'),'args':['--native',str(files[0])],'tokenizer':str(ROOT/'tokenizer'),'backend':'metal'}))
    return 0
"#).unwrap();
        let before = fs::read(&files[0]).unwrap();
        s.use_local(files[0].to_str().unwrap(), true).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while s.preparation.lock().await.busy() {
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(s.preparation.lock().await.status, "completed");
        assert!(s.settings.read().await.config.ends_with("strata-q2_0.json"));
        assert_eq!(before, fs::read(&files[0]).unwrap());
        assert_eq!(fs::read_dir(external).unwrap().count(), 2);
        assert!(s.process.lock().await.child.is_none());
        fs::remove_dir_all(base).unwrap();
    }
}
