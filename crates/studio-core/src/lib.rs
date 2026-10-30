pub mod catalog;
pub mod conversations;
pub mod docking;
pub mod engine;
pub mod inference;
pub mod layout;
pub mod local_model;
pub mod memory_usage;
pub mod metal_policy;
pub mod omni;
pub mod omni_runtime;
pub mod omni_realtime;
mod owned_process;
pub mod preparation;
mod token_budget;
pub mod vllm_inspect;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{Mutex, RwLock},
};
use tower_http::{cors::CorsLayer, services::ServeDir};

fn default_chat_font_size() -> u8 {
    13
}
fn default_theme() -> String {
    "dark".into()
}
fn valid_appearance(size: u8, theme: &str) -> bool {
    (12..=32).contains(&size) && matches!(theme, "light" | "dark")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub engine: engine::Engine,
    #[serde(default)]
    pub vllm: engine::VllmSettings,
    #[serde(default)]
    pub omni: omni_runtime::Settings,
    pub strata_root: String,
    pub python: String,
    pub config: String,
    pub mock: bool,
    pub temperature: f64,
    pub max_tokens: u32,
    pub language: String,
    #[serde(default = "default_chat_font_size")]
    pub chat_font_size: u8,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub model_directories: Vec<String>,
    #[serde(default)]
    pub metal_memory: metal_policy::MemorySettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: engine::Engine::default(),
            vllm: engine::VllmSettings::default(),
            omni: omni_runtime::Settings::default(),
            strata_root: String::new(),
            python: "python3".into(),
            config: String::new(),
            mock: false,
            temperature: 0.7,
            max_tokens: 512,
            language: "ja".into(),
            chat_font_size: default_chat_font_size(),
            theme: default_theme(),
            model_directories: Vec::new(),
            metal_memory: metal_policy::MemorySettings::default(),
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<omni_runtime::Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<inference::Inference>,
    pub role: String,
    pub content: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub conversations: conversations::Library,
    pub messages: Vec<Message>,
    pub detached: Vec<String>,
    #[serde(default)]
    pub generating: bool,
    #[serde(default)]
    pub layout: layout::Layout,
    #[serde(skip)]
    pub workspace_hosts: std::collections::BTreeMap<String, String>,
}
struct Startup {
    started: std::time::Instant,
    control_ms: Option<u64>,
    inference_ms: Option<u64>,
    counter_ms: Option<u64>,
}
impl Startup {
    fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
            control_ms: None,
            inference_ms: None,
            counter_ms: None,
        }
    }
    fn snapshot(&self) -> Value {
        json!({"controlReadyObservedMs":self.control_ms,"inferenceReadyObservedMs":self.inference_ms,
            "counterSpawnedMs":self.counter_ms,"modelLoadingObservedMs":self.inference_ms.zip(self.control_ms).map(|(ready,control)|ready.saturating_sub(control))})
    }
}
struct Process {
    child: Option<Child>,
    key: String,
    port: u16,
    memory: Option<Value>,
    group: Option<u32>,
    workers: Vec<owned_process::Identity>,
    startup: Option<Startup>,
}
pub struct Studio {
    pub root: PathBuf,
    pub port: u16,
    pub token: String,
    pub settings: RwLock<Settings>,
    pub document: RwLock<Document>,
    pub docking_diagnostics: Mutex<docking::DragDiagnostics>,
    process: Mutex<Process>,
    logs: Arc<Mutex<VecDeque<String>>>,
    client: reqwest::Client,
    storage: Mutex<Option<PathBuf>>,
    memory_bytes: Option<u64>,
    memory_usage: Mutex<memory_usage::Monitor>,
    requests: Mutex<VecDeque<inference::Metrics>>,
    preparation: Mutex<preparation::Preparation>,
    lifecycle: Mutex<()>,
    catalog: Mutex<Option<catalog::CachedScan>>,
    vllm_inspection: Mutex<Option<vllm_inspect::Cached>>,
    token_counter: Mutex<token_budget::Counter>,
    omni_assets: Mutex<()>,
    omni_counter_failed: std::sync::atomic::AtomicBool,
    active_requests: AtomicUsize,
}
impl Studio {
    pub fn new(root: PathBuf, port: u16) -> Self {
        let mut settings = Settings::default();
        settings.strata_root = root.to_string_lossy().into();
        if root.join(".venv/bin/python").exists() {
            settings.python = root.join(".venv/bin/python").to_string_lossy().into();
        } else if cfg!(target_os = "macos") {
            for candidate in ["/opt/homebrew/bin/python3", "/usr/local/bin/python3"] {
                if PathBuf::from(candidate).exists() {
                    settings.python = candidate.into();
                    break;
                }
            }
        }
        if let Ok(files) = std::fs::read_dir(&root) {
            let mut configs: Vec<_> = files
                .flatten()
                .map(|f| f.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("strata-"))
                        && p.extension().is_some_and(|x| x == "json")
                })
                .collect();
            configs.sort();
            if let Some(p) = configs.first() {
                settings.config = p.to_string_lossy().into();
            }
        }
        let vllm_root = root.parent().unwrap_or(&root).join("vLLM-apple");
        settings.vllm.root = vllm_root.to_string_lossy().into();
        for env in [".venv", ".venv-vllm-metal-031"] {
            let python = vllm_root.join(env).join("bin/python");
            if python.is_file() {
                settings.vllm.python = python.to_string_lossy().into();
                break;
            }
        }
        Self {
            root,
            port,
            token: uuid::Uuid::new_v4().to_string(),
            settings: RwLock::new(settings),
            document: RwLock::new(Document::default()),
            docking_diagnostics: Mutex::new(docking::DragDiagnostics::default()),
            process: Mutex::new(Process {
                child: None,
                key: String::new(),
                port: 0,
                memory: None,
                group: None,
                workers: Vec::new(),
                startup: None,
            }),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            client: reqwest::Client::new(),
            storage: Mutex::new(None),
            preparation: Mutex::new(preparation::Preparation::default()),
            lifecycle: Mutex::new(()),
            catalog: Mutex::new(None),
            vllm_inspection: Mutex::new(None),
            token_counter: Mutex::new(token_budget::Counter::default()),
            omni_assets: Mutex::new(()),
            omni_counter_failed: std::sync::atomic::AtomicBool::new(false),
            active_requests: AtomicUsize::new(0),
            memory_usage: Mutex::new(memory_usage::Monitor::default()),
            requests: Mutex::new(VecDeque::new()),
            memory_bytes: if cfg!(target_os = "macos") {
                std::process::Command::new("/usr/sbin/sysctl")
                    .args(["-n", "hw.memsize"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
            } else {
                None
            },
        }
    }
    pub fn with_storage(mut self, path: PathBuf) -> Self {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                if let Ok(settings) = serde_json::from_value::<Settings>(value["settings"].clone())
                {
                    let mut settings = settings;
                    if settings.vllm.root.is_empty() {
                        settings.vllm = self.settings.get_mut().vllm.clone();
                    }
                    *self.settings.get_mut() = settings;
                }
                if let Ok(mut doc) = serde_json::from_value::<Document>(value["document"].clone()) {
                    if doc.layout.validate().is_err() {
                        doc.layout = layout::Layout::default();
                    }
                    inference::mark_interrupted(&mut doc.messages);
                    doc.conversations.restore_runtime();
                    doc.generating = false;
                    doc.detached.clear(); // Native windows do not survive a process restart.
                    *self.document.get_mut() = doc;
                }
            }
        }
        *self.storage.get_mut() = Some(path);
        self
    }
    /// Read the previous product's state once, then save only under the new identity.
    pub fn with_legacy_storage(mut self, path: PathBuf, legacy: PathBuf) -> Self {
        let source = if path.exists() { path.clone() } else { legacy };
        self = self.with_storage(source);
        *self.storage.get_mut() = Some(path);
        self
    }
    pub async fn persist(&self) -> Result<(), String> {
        let storage = self.storage.lock().await;
        if let Some(path) = storage.as_ref() {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let data = serde_json::to_vec(&json!({"settings":*self.settings.read().await,"document":*self.document.read().await})).map_err(|e|e.to_string())?;
            let tmp = path.with_extension("tmp");
            tokio::fs::write(&tmp, data)
                .await
                .map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            tokio::fs::rename(tmp, path)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub async fn stop(&self) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_inner().await
    }
    async fn stop_inner(&self) -> Result<(), String> {
        self.token_counter.lock().await.stop().await?;
        let mut p = self.process.lock().await;
        if let Some(id) = p.child.as_ref().and_then(|c| c.id()) {
            let captured = owned_process::capture(id)?;
            p.workers.retain(|worker| worker.alive());
            for worker in captured {
                if !p.workers.contains(&worker) {
                    p.workers.push(worker);
                }
            }
        }
        if let Some(mut c) = p.child.take() {
            if p.memory.is_some() {
                self.memory_usage
                    .lock()
                    .await
                    .refresh(self.memory_bytes, None);
            }
            let group = p.group;
            #[cfg(unix)]
            if let Some(id) = group {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGTERM);
                }
            }
            if tokio::time::timeout(Duration::from_secs(8), c.wait())
                .await
                .is_err()
            {
                let _ = c.start_kill();
                let _ = c.wait().await;
            }
            #[cfg(unix)]
            if let Some(id) = group {
                // Native workers may survive their Python parent. Reap the owned group
                // before another engine is allowed to allocate model memory.
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
                for _ in 0..100 {
                    if unsafe { libc::kill(-(id as i32), 0) } == -1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                if unsafe { libc::kill(-(id as i32), 0) } == 0 {
                    p.child = Some(c);
                    return Err(
                        "Engine workers have not exited; retry stop before switching".into(),
                    );
                }
            }
            for worker in &p.workers {
                worker.kill();
            }
            for _ in 0..100 {
                if !p.workers.iter().any(|worker| worker.alive()) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            if p.workers.iter().any(|worker| worker.alive()) {
                p.child = Some(c);
                return Err(
                    "Native engine workers have not exited; retry stop before switching".into(),
                );
            }
            p.workers.clear();
            p.group = None;
            push_log(&self.logs, "[studio] Process stopped".into()).await;
        }
        Ok(())
    }
    pub async fn switch_engine(&self, target: engine::Engine) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.preparation.lock().await.busy()
            || self.document.read().await.generating
            || self.active_requests.load(Ordering::SeqCst) != 0
        {
            return Err("Wait for active requests and model preparation to finish".into());
        }
        if self.settings.read().await.engine == target {
            return Ok(());
        }
        self.stop_inner().await?;
        let previous = self.settings.read().await.engine;
        self.settings.write().await.engine = target;
        if let Err(error) = self.persist().await {
            self.settings.write().await.engine = previous;
            return Err(error);
        }
        push_log(
            &self.logs,
            format!("[soliton] Selected {} (stopped)", target.label()),
        )
        .await;
        Ok(())
    }
    async fn check_omni(
        &self,
        port: u16,
        key: &str,
        body: &Value,
        context: u32,
    ) -> Result<u64, omni_runtime::AdmissionError> {
        if self.omni_counter_failed.load(Ordering::SeqCst) {
            return Err(omni_runtime::AdmissionError::Counter(
                "Omni processor counting failed; stop and restart the engine",
            ));
        }
        let result = omni_runtime::check(&self.client, port, key, body, context).await;
        if matches!(result, Err(omni_runtime::AdmissionError::Counter(_))) {
            self.omni_counter_failed.store(true, Ordering::SeqCst);
        }
        result
    }
    pub async fn start(&self) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        self.start_inner().await
    }
    async fn start_inner(&self) -> Result<(), String> {
        if self.preparation.lock().await.busy() {
            return Err("Wait for model preparation to finish".into());
        }
        {
            let mut p = self.process.lock().await;
            if let Some(c) = p.child.as_mut() {
                if c.try_wait().map_err(|e| e.to_string())?.is_none() {
                    return Err("Engine is already running".into());
                }
            }
        }
        self.stop_inner().await?;
        let mut p = self.process.lock().await;
        let s = self.settings.read().await.clone();
        p.startup = None;
        if s.engine == engine::Engine::Lfm3Omni {
            let reserve = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
            let port = reserve.local_addr().map_err(|e| e.to_string())?.port();
            let key = uuid::Uuid::new_v4().to_string();
            let config = s.omni.clone();
            let command_key = key.clone();
            let mut cmd = tokio::task::spawn_blocking(move || config.command(port, &command_key))
                .await
                .map_err(|e| e.to_string())??;
            cmd.kill_on_drop(true)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(unix)]
            cmd.process_group(0);
            drop(reserve);
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("Cannot launch Omni: {e}"))?;
            if let Some(out) = child.stdout.take() {
                pump(out, self.logs.clone());
            }
            if let Some(out) = child.stderr.take() {
                pump(out, self.logs.clone());
            }
            p.startup = Some(Startup::new());
            p.group = child.id();
            p.child = Some(child);
            p.port = port;
            p.key = key;
            p.memory = Some(json!({"engine":"lfm3Omni"}));
            self.omni_counter_failed.store(false, Ordering::SeqCst);
            push_log(
                &self.logs,
                "[soliton] Starting exclusive LFM3-Omni runtime".into(),
            )
            .await;
            return Ok(());
        }
        if s.engine == engine::Engine::VllmApple {
            let reserve = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
            let backend_reserve =
                std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
            let port = reserve.local_addr().map_err(|e| e.to_string())?.port();
            let backend_port = backend_reserve
                .local_addr()
                .map_err(|e| e.to_string())?
                .port();
            let key = uuid::Uuid::new_v4().to_string();
            let mut cmd = s.vllm.command(port, backend_port, &key)?;
            cmd.kill_on_drop(true)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(unix)]
            cmd.process_group(0);
            drop(reserve);
            drop(backend_reserve);
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("Cannot launch vLLM-Apple: {e}"))?;
            if let Some(out) = child.stdout.take() {
                pump(out, self.logs.clone());
            }
            if let Some(out) = child.stderr.take() {
                pump(out, self.logs.clone());
            }
            p.startup = Some(Startup::new());
            p.group = child.id();
            p.child = Some(child);
            p.port = port;
            p.key = key;
            p.memory = Some(json!({"engine":"vllmApple"}));
            push_log(
                &self.logs,
                format!(
                    "[soliton] Starting vLLM-Apple / {} on private port {port}",
                    s.vllm.backend
                ),
            )
            .await;
            return Ok(());
        }
        let root = PathBuf::from(&s.strata_root);
        if !root.join("serve/server.py").exists() {
            return Err("Strata/serve/server.py not found; set STRATA_ROOT".into());
        }
        if !s.mock {
            if s.config.is_empty() || !PathBuf::from(&s.config).is_file() {
                return Err(
                    "Select an existing strata-*.json config produced by Strata/setup.sh".into(),
                );
            }
        }
        if !s.mock {
            let info = preparation::inspect_config(&PathBuf::from(&s.config), &root);
            if !info.valid {
                return Err(info.error.unwrap_or("Invalid Strata engine config".into()));
            }
            if !info.engine_exists {
                return Err("Configured engine executable was not found".into());
            }
            if !info.tokenizer_exists {
                return Err("Configured tokenizer/vocab.json was not found".into());
            }
            if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
                if let Some(profile) = metal_policy::apply_with_settings(
                    &PathBuf::from(&s.config),
                    self.memory_bytes,
                    &s.metal_memory,
                )? {
                    push_log(
                        &self.logs,
                        format!(
                            "[studio] Metal memory settings: context {}, prompt chunk {}",
                            profile.context, profile.prompt_chunk
                        ),
                    )
                    .await;
                }
            }
        }
        if !s.mock {
            let config: Value =
                serde_json::from_slice(&std::fs::read(&s.config).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if config["backend"] == "strata-gemma3-metal" {
                let reserve =
                    std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
                let port = reserve.local_addr().map_err(|e| e.to_string())?.port();
                let key = uuid::Uuid::new_v4().to_string();
                let args: Vec<&str> = config["args"]
                    .as_array()
                    .ok_or("Missing Gemma arguments")?
                    .iter()
                    .map(|v| v.as_str().ok_or("Invalid Gemma argument"))
                    .collect::<Result<_, _>>()?;
                let mut cmd =
                    Command::new(config["exe"].as_str().ok_or("Missing Gemma executable")?);
                cmd.current_dir(&root)
                    .args(args)
                    .args(["--host", "127.0.0.1", "--port", &port.to_string()])
                    .env("LLAMA_API_KEY", &key)
                    .kill_on_drop(true)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                #[cfg(unix)]
                cmd.process_group(0);
                drop(reserve);
                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("Cannot launch bundled Gemma Metal backend: {e}"))?;
                if let Some(out) = child.stdout.take() {
                    pump(out, self.logs.clone());
                }
                if let Some(out) = child.stderr.take() {
                    pump(out, self.logs.clone());
                }
                p.port = port;
                p.key = key;
                p.group = child.id();
                p.child = Some(child);
                p.memory = Some(json!({"engine":"strata-gemma3-metal","context":2048}));
                push_log(
                    &self.logs,
                    "[soliton] Starting bundled Strata Gemma 3 / llama.cpp Metal (text)".into(),
                )
                .await;
                return Ok(());
            }
        }
        let probe = tokio::time::timeout(Duration::from_secs(10), Command::new(&s.python)
            .args(["-c", "import sys; assert sys.version_info >= (3,10), 'Python 3.10+ required'; import jinja2, regex, numpy, PIL"])
            .kill_on_drop(true).output()).await.map_err(|_| "Python check timed out")?
            .map_err(|e| format!("Cannot launch Python: {e}"))?;
        if !probe.status.success() {
            return Err(format!(
                "Python 3.10+ and Strata dependencies required: {}",
                String::from_utf8_lossy(&probe.stderr)
                    .chars()
                    .take(1000)
                    .collect::<String>()
            ));
        }
        // Reserve an ephemeral loopback port; do not attach to somebody else's engine.
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        p.port = reserve.local_addr().map_err(|e| e.to_string())?.port();
        p.key = uuid::Uuid::new_v4().to_string();
        let mut cmd = Command::new(&s.python);
        cmd.current_dir(&root)
            .args([
                "-u",
                "-m",
                "serve.server",
                "--host",
                "127.0.0.1",
                "--port",
                &p.port.to_string(),
                "--engine",
                if s.mock { "mock" } else { "strata" },
            ])
            .env("STRATA_API_KEY", &p.key)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if !s.mock {
            cmd.args(["--config", &s.config]);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        drop(reserve);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Cannot launch {}: {e}", s.python))?;
        if let Some(out) = child.stdout.take() {
            pump(out, self.logs.clone());
        }
        if let Some(out) = child.stderr.take() {
            pump(out, self.logs.clone());
        }
        p.memory = if s.mock {
            None
        } else {
            metal_policy::preview(
                &PathBuf::from(&s.config),
                self.memory_bytes,
                &s.metal_memory,
            )
        };
        p.group = child.id();
        p.child = Some(child);
        if !s.mock {
            self.memory_usage
                .lock()
                .await
                .refresh(self.memory_bytes, p.child.as_ref().and_then(|c| c.id()));
        }
        push_log(
            &self.logs,
            format!(
                "[studio] Starting {} on private port {}",
                if s.mock {
                    "test engine (no model)"
                } else {
                    "Strata"
                },
                p.port
            ),
        )
        .await;
        Ok(())
    }
    async fn engine_pid(&self) -> Option<u32> {
        let mut p = self.process.lock().await;
        if p.memory.is_none() {
            return None;
        } // Mock engines are not Strata.
        let c = p.child.as_mut()?;
        if !matches!(c.try_wait(), Ok(None)) {
            return None;
        }
        let id = c.id();
        if let Some(id) = id {
            if let Ok(workers) = owned_process::capture(id) {
                p.workers = workers;
            }
        }
        id
    }
    async fn capture_memory(&self) {
        let pid = self.engine_pid().await;
        self.memory_usage
            .lock()
            .await
            .capture(self.memory_bytes, pid);
    }
    pub async fn apply_memory_restart(
        &self,
        memory: metal_policy::MemorySettings,
    ) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.settings.read().await.engine != engine::Engine::Strata {
            return Err("Metal memory controls apply to Strata only".into());
        }
        let selected = self.settings.read().await.clone();
        if preparation::inspect_config(
            std::path::Path::new(&selected.config),
            std::path::Path::new(&selected.strata_root),
        )
        .backend
            == "strata-gemma3-metal"
        {
            return Err(
                "Gemma 3 uses its prepared context (2048); Qwen memory controls do not apply"
                    .into(),
            );
        }
        if self.active_requests.load(Ordering::SeqCst) != 0 {
            return Err("Wait for active API requests to finish".into());
        }
        if !memory.valid(self.memory_bytes) {
            return Err("Invalid Metal memory settings".into());
        }
        if self.preparation.lock().await.busy() {
            return Err("Wait for model preparation to finish".into());
        }
        if self.document.read().await.generating {
            return Err("Wait for the current reply to finish".into());
        }
        let previous = self.settings.read().await.metal_memory.clone();
        self.capture_memory().await;
        self.memory_usage.lock().await.event(
            "settings",
            Some(if memory.automatic {
                metal_policy::automatic(self.memory_bytes)["context"]
                    .as_u64()
                    .unwrap_or(memory.context as u64) as u32
            } else {
                memory.context
            }),
        );
        self.stop_inner().await?;
        self.settings.write().await.metal_memory = memory;
        if let Err(error) = self.persist().await {
            self.settings.write().await.metal_memory = previous;
            let recovery = self.start_inner().await;
            return Err(format!(
                "Cannot save memory settings: {error}; previous engine recovery: {}",
                recovery.err().unwrap_or_else(|| "ok".into())
            ));
        }
        self.start_inner()
            .await
            .map_err(|e| format!("Memory settings saved, but engine restart failed: {e}"))
    }
    async fn record_request(&self, metrics: inference::Metrics) {
        let mut records = self.requests.lock().await;
        if records.len() >= 100 {
            records.pop_front();
        }
        records.push_back(metrics);
    }
    pub async fn snapshot(&self) -> Value {
        let (running, upstream, key) = {
            let mut p = self.process.lock().await;
            let running = p
                .child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)));
            (running, p.port, p.key.clone())
        };
        if !running {
            if let Ok(_lifecycle) = self.lifecycle.try_lock() {
                let live = self
                    .process
                    .lock()
                    .await
                    .child
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(None)));
                if !live {
                    let _ = self.token_counter.lock().await.stop().await;
                }
            }
        }
        let health = if running {
            self.client
                .get(format!("http://127.0.0.1:{upstream}/health"))
                .bearer_auth(&key)
                .timeout(Duration::from_secs(1))
                .send()
                .await
                .ok()
                .filter(|h| h.status().is_success())
        } else {
            None
        };
        let health = match health {
            Some(h) => h.json::<Value>().await.ok(),
            None => None,
        };
        let settings = self.settings.read().await.clone();
        // A health response is observed outside the lifecycle lock. Recheck the
        // exact owned session before preparing anything; never act on stale health.
        if settings.engine == engine::Engine::VllmApple
            && health.as_ref().is_some_and(|h| h["control_ready"] == true)
        {
            if let Ok(_lifecycle) = self.lifecycle.try_lock() {
                let current = self.settings.read().await.clone();
                let mut process = self.process.lock().await;
                if current.engine == settings.engine
                    && current.vllm == settings.vllm
                    && process.port == upstream
                    && process.key == key
                    && process
                        .child
                        .as_mut()
                        .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
                {
                    if let Some(startup) = process.startup.as_mut() {
                        let elapsed = startup.started.elapsed().as_millis() as u64;
                        startup.control_ms.get_or_insert(elapsed);
                        if health
                            .as_ref()
                            .is_some_and(|h| h["inference_ready"] == true)
                        {
                            startup.inference_ms.get_or_insert(elapsed);
                        }
                        if let Ok(mut counter) = self.token_counter.try_lock() {
                            if matches!(counter.prepare(&settings), Ok(true)) {
                                startup.counter_ms = Some(elapsed);
                            }
                        }
                    }
                }
            }
        }
        let startup = if running {
            self.process
                .lock()
                .await
                .startup
                .as_ref()
                .map(Startup::snapshot)
        } else {
            None
        };
        let memory_tuning = json!({"automatic":metal_policy::automatic(self.memory_bytes),"next":metal_policy::preview(&PathBuf::from(&settings.config),self.memory_bytes,&settings.metal_memory),"active":if running { self.process.lock().await.memory.clone() } else { None }});
        let status = if !running {
            "offline"
        } else if health.as_ref().is_some_and(|h| {
            settings.engine == engine::Engine::Strata
                || h["inference_ready"] == true
                || (settings.engine == engine::Engine::Lfm3Omni && h["status"] == "ok")
        }) {
            "ready"
        } else {
            "starting"
        };
        let pid = self.engine_pid().await;
        let memory_usage = self
            .memory_usage
            .lock()
            .await
            .snapshot(self.memory_bytes, pid);
        let document = {
            let d = self.document.read().await;
            json!({"messages":d.messages,"detached":d.detached,"generating":d.generating,"layout":d.layout,"workspaceGroups":d.layout.groups(),"workspaceHosts":d.workspace_hosts,"conversations":d.conversations.summaries(&d.messages)})
        };
        let requests = self
            .requests
            .lock()
            .await
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        let vllm_inspection = self.inspection_snapshot(&settings.vllm).await;
        json!({"omniCounterFailed":self.omni_counter_failed.load(Ordering::SeqCst),"startup":startup,"vllmInspection":vllm_inspection,"dockingDiagnostics":*self.docking_diagnostics.lock().await,"requests":requests,"memoryUsage":memory_usage,"memoryTuning":memory_tuning,"preparation":self.preparation.lock().await.snapshot(),"status":status,"health":health,"settings":*self.settings.read().await,"document":document,"logs":*self.logs.lock().await,"architecture":std::env::consts::ARCH,"memoryBytes":self.memory_bytes,"selectedEngine":settings.engine,"backend":if settings.engine == engine::Engine::Lfm3Omni {"LFM3-Omni"}else if settings.engine == engine::Engine::VllmApple {"vLLM-Apple"} else if preparation::inspect_config(std::path::Path::new(&settings.config), std::path::Path::new(&settings.strata_root)).backend == "strata-gemma3-metal" {"Gemma 3 Metal"} else if cfg!(all(target_os="macos",target_arch="aarch64")){"Metal"}else{"Strata"},"endpoint":format!("http://127.0.0.1:{}/v1",self.port),"root":self.root})
    }
}
fn pump<T: tokio::io::AsyncRead + Unpin + Send + 'static>(
    stream: T,
    logs: Arc<Mutex<VecDeque<String>>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            push_log(&logs, line.chars().take(4096).collect()).await;
        }
    });
}
async fn push_log(logs: &Mutex<VecDeque<String>>, line: String) {
    let mut l = logs.lock().await;
    if l.len() >= 300 {
        l.pop_front();
    }
    l.push_back(line);
}
fn error(status: StatusCode, msg: impl ToString) -> Response {
    (
        status,
        Json(json!({"error":{"message":msg.to_string(),"type":"studio_error"}})),
    )
        .into_response()
}
fn authorized(h: &HeaderMap, s: &Studio) -> bool {
    h.get("x-studio-token").and_then(|x| x.to_str().ok()) == Some(&s.token)
}
async fn session(State(s): State<Arc<Studio>>, h: HeaderMap) -> Response {
    let origin = h.get("origin").and_then(|x| x.to_str().ok()).unwrap_or("");
    if origin != format!("http://127.0.0.1:{}", s.port) && origin != "http://127.0.0.1:5180" {
        return error(StatusCode::FORBIDDEN, "Untrusted origin");
    }
    Json(json!({"token":s.token,"url":format!("http://127.0.0.1:{}",s.port)})).into_response()
}
async fn inspect(State(s): State<Arc<Studio>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    Json(s.inspect().await).into_response()
}
async fn snapshot(State(s): State<Arc<Studio>>, h: HeaderMap) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    Json(s.snapshot().await).into_response()
}
async fn action(State(s): State<Arc<Studio>>, h: HeaderMap, Json(v): Json<Value>) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    if v["action"] == "traceDocking" {
        let mut trace = s.docking_diagnostics.lock().await;
        trace.enabled = v["enabled"].as_bool().unwrap_or(false);
        trace.events.clear();
        return Json(json!({"ok":true})).into_response();
    }
    let result = match v["action"].as_str().unwrap_or("") {
        "switchEngine" => match serde_json::from_value(v["engine"].clone()) {
            Ok(engine) => s.switch_engine(engine).await,
            Err(_) => Err("Invalid engine".into()),
        },
        "inspectVllm" => s.inspect_vllm().await,
        "start" => s.start().await,
        "applyMemoryRestart" => match serde_json::from_value(v["memory"].clone()) {
            Ok(memory) => s.apply_memory_restart(memory).await,
            Err(_) => Err("Invalid Metal memory settings".into()),
        },
        "prepare" => {
            s.prepare(
                v["model"].as_str().unwrap_or(""),
                v["confirmed"].as_bool().unwrap_or(false),
            )
            .await
        }
        "inspectLocalModel" => {
            return match s.inspect_local(v["path"].as_str().unwrap_or("")).await {
                Ok(info) => Json(info).into_response(),
                Err(e) => error(StatusCode::BAD_REQUEST, e),
            };
        }
        "useLocalModel" => {
            s.use_local(
                v["path"].as_str().unwrap_or(""),
                v["confirmed"].as_bool().unwrap_or(false),
            )
            .await
        }
        "cancelPreparation" => s.cancel_preparation().await,
        "workspaceTab" | "moveWorkspaceTab" => {
            let tab = v["tab"].as_str().unwrap_or("");
            let before = if v["action"] == "moveWorkspaceTab" {
                Some(v["before"].as_str().unwrap_or(""))
            } else {
                None
            };
            s.document.write().await.layout.workspace(tab, before)
        }
        "dockWorkspaceTab" | "resetWorkspace" => {
            let mut d = s.document.write().await;
            // Native windows must return to their slots before changing group membership.
            if d.detached
                .iter()
                .any(|id| id == "workspace" || id.starts_with("group-"))
            {
                Err("Dock floating workspace groups before changing the split".into())
            } else if v["action"] == "dockWorkspaceTab" {
                d.layout.dock(
                    v["tab"].as_str().unwrap_or(""),
                    v["target"].as_str().unwrap_or(""),
                    v["placement"].as_str().unwrap_or(""),
                )
            } else {
                d.layout.workspace_order = layout::Layout::default().workspace_order;
                d.layout.workspace_active = "chat".into();
                d.layout.workspace_split = None;
                d.layout.workspace_ratio = 50;
                Ok(())
            }
        }
        "resizeWorkspace" => s
            .document
            .write()
            .await
            .layout
            .resize_workspace(v["ratio"].as_u64().unwrap_or(0)),
        "layout" => {
            let mut document = s.document.write().await;
            match document.layout.patch(&v["layout"]) {
                Ok(layout) => {
                    document.layout = layout;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        "stop" => s.stop().await,
        "appearance" => {
            let mut settings = s.settings.write().await;
            let size = v
                .get("chatFontSize")
                .map(|n| n.as_u64().filter(|n| *n <= 255).map(|n| n as u8));
            let theme = v.get("theme").map(|n| n.as_str());
            match (size, theme) {
                (Some(None), _) | (_, Some(None)) => Err("Invalid appearance settings".into()),
                _ => {
                    let size = size.flatten().unwrap_or(settings.chat_font_size);
                    let theme = theme.flatten().unwrap_or(&settings.theme).to_string();
                    if valid_appearance(size, &theme) {
                        settings.chat_font_size = size;
                        settings.theme = theme;
                        Ok(())
                    } else {
                        Err("Invalid appearance settings".into())
                    }
                }
            }
        }
        "settings" => match serde_json::from_value::<Settings>(v["settings"].clone()) {
            Ok(settings)
                if valid_appearance(settings.chat_font_size, &settings.theme)
                    && settings.max_tokens > 0
                    && settings.max_tokens <= 262144
                    && settings.temperature.is_finite()
                    && (0.0..=2.0).contains(&settings.temperature)
                    && ["en", "ja", "zh"].contains(&settings.language.as_str())
                    && catalog::valid_directories(&settings.model_directories)
                    && settings.metal_memory.valid(s.memory_bytes)
                    && settings.vllm.valid()
                    && (settings.engine != engine::Engine::Lfm3Omni || settings.omni.valid()) =>
            {
                let _lifecycle = s.lifecycle.lock().await;
                if s.preparation.lock().await.busy() {
                    return error(
                        StatusCode::CONFLICT,
                        "Settings are locked during model preparation",
                    );
                }
                let current = s.settings.read().await.clone();
                if settings.engine != current.engine {
                    return error(StatusCode::CONFLICT, "Use switchEngine to change engines");
                }
                if settings.vllm != current.vllm || settings.omni != current.omni {
                    let mut process = s.process.lock().await;
                    if process
                        .child
                        .as_mut()
                        .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
                    {
                        return error(
                            StatusCode::CONFLICT,
                            "Stop the engine before changing vLLM-Apple settings",
                        );
                    }
                }
                if settings.metal_memory != current.metal_memory {
                    let mut process = s.process.lock().await;
                    if process
                        .child
                        .as_mut()
                        .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
                    {
                        return error(
                            StatusCode::CONFLICT,
                            "Stop the engine before changing memory settings",
                        );
                    }
                }
                if settings.metal_memory != s.settings.read().await.metal_memory {
                    s.capture_memory().await;
                    s.memory_usage
                        .lock()
                        .await
                        .event("settings", Some(settings.metal_memory.context));
                }
                if settings.vllm != current.vllm {
                    if let Err(e) = s.token_counter.lock().await.stop().await {
                        return error(StatusCode::CONFLICT, e);
                    }
                    *s.vllm_inspection.lock().await = None;
                }
                *s.settings.write().await = settings;
                Ok(())
            }
            _ => Err("Invalid settings".into()),
        },
        "newConversation" | "switchConversation" | "deleteConversation" => {
            let mut d = s.document.write().await;
            conversations::change(
                &mut d,
                v["action"].as_str().unwrap(),
                v["id"].as_str().unwrap_or(""),
            )
        }
        "messages" => match serde_json::from_value::<Vec<Message>>(v["messages"].clone()) {
            Ok(m)
                if m.len() <= 1000
                    && m.iter().all(|x| {
                        ["system", "user", "assistant"].contains(&x.role.as_str())
                            && x.content.len() <= 1_000_000
                    }) =>
            {
                let mut d = s.document.write().await;
                if d.generating {
                    return error(StatusCode::CONFLICT, "Conversation is generating");
                }
                d.messages = m;
                d.conversations.touch();
                Ok(())
            }
            _ => Err("Invalid messages".into()),
        },
        "detach" | "dock" => {
            let panel = v["panel"].as_str().unwrap_or("");
            if !["engine", "api", "logs"].contains(&panel) {
                Err("Invalid panel".into())
            } else {
                let mut d = s.document.write().await;
                d.detached.retain(|x| x != panel);
                if v["action"] == "detach" {
                    d.detached.push(panel.into());
                }
                Ok(())
            }
        }
        "clearLogs" => {
            s.logs.lock().await.clear();
            Ok(())
        }
        _ => Err("Unknown action".into()),
    };
    match result {
        Ok(()) => match s.persist().await {
            Ok(()) => Json(json!({"ok":true})).into_response(),
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
async fn proxy(State(s): State<Arc<Studio>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    // Inference stays loopback-only; Studio clients authenticate with the process session token.
    let authenticated = authorized(&parts.headers, &s)
        || parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            == Some(format!("Bearer {}", s.token).as_str())
        || parts.headers.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(&s.token);
    if !authenticated {
        return error(StatusCode::UNAUTHORIZED, "Use the Studio session API key");
    }
    let lifecycle = s.lifecycle.lock().await;
    let (port, key) = {
        let mut p = s.process.lock().await;
        if !p
            .child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        {
            return error(StatusCode::SERVICE_UNAVAILABLE, "Engine is offline");
        }
        (p.port, p.key.clone())
    };
    let path = parts
        .uri
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("/");
    let mut body = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "Request exceeds 16 MiB"),
    };
    let omni_config = s.settings.read().await.clone();
    if omni_config.engine == engine::Engine::Lfm3Omni {
        if s.active_requests.load(Ordering::SeqCst) != 0 {
            return error(
                StatusCode::CONFLICT,
                "Omni inference or media processing is busy",
            );
        }
        if path.split('?').next() != Some("/v1/chat/completions")
            && path.split('?').next() != Some("/v1/models")
        {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Unqualified Omni API mode",
            );
        }
        if path.split('?').next() == Some("/v1/chat/completions") {
            let mut v: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid Omni JSON"),
            };
            let messages: Vec<Message> = match serde_json::from_value(v["messages"].clone()) {
                Ok(v) => v,
                Err(_) => {
                    return error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "Use text and Rust-owned Omni attachment IDs",
                    )
                }
            };
            v["messages"] = match omni_runtime::messages(&messages, &omni_config.omni) {
                Ok(v) => json!(v),
                Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, e),
            };
            v["temperature"] = json!(0);
            if let Err(e) = s.check_omni(port, &key, &v, omni_config.omni.context).await {
                return error(e.status(), e);
            }
            body = axum::body::Bytes::from(serde_json::to_vec(&v).unwrap());
        }
    }
    if matches!(
        path.split('?').next(),
        Some("/v1/chat/completions" | "/v1/completions")
    ) {
        let settings = s.settings.read().await.clone();
        let value = match serde_json::from_slice::<Value>(&body) {
            Ok(v) => v,
            Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid inference JSON"),
        };
        if let Err(rejection) = s.token_counter.lock().await.check(&settings, &value).await {
            return rejection.response();
        }
    }
    let model = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v["model"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut trace = ProxyTrace {
        studio: s.clone(),
        tracker: inference::Tracker::new(
            if matches!(
                path.split('?').next(),
                Some("/v1/chat/completions" | "/v1/completions" | "/v1/messages")
            ) {
                "api"
            } else {
                "skip"
            },
            &model,
        ),
        json: false,
        eof: false,
    };
    trace.tracker.info.metrics.engine = Some(s.settings.read().await.engine.label().into());
    let mut request = s
        .client
        .request(parts.method, format!("http://127.0.0.1:{port}{path}"))
        .bearer_auth(key)
        .body(body);
    for name in ["content-type", "anthropic-version", "anthropic-beta"] {
        if let Some(value) = parts.headers.get(name) {
            request = request.header(name, value);
        }
    }
    let inference = InferenceGuard::new(s.clone()).await;
    drop(lifecycle);
    let upstream = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            trace.fail();
            return error(StatusCode::BAD_GATEWAY, e);
        }
    };
    let status = upstream.status();
    if !status.is_success() {
        trace.fail();
    }
    trace.json = upstream
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.contains("application/json"));
    let headers = upstream.headers().clone();
    use tokio_stream::StreamExt;
    let stream = upstream
        .bytes_stream()
        .map(Some)
        .chain(tokio_stream::once(None))
        .map(move |event| {
            let _ = &inference;
            match event {
                Some(chunk) => {
                    match &chunk {
                        Ok(bytes) => trace.feed(bytes),
                        Err(_) => trace.fail(),
                    };
                    chunk
                }
                None => {
                    trace.eof = true;
                    Ok(axum::body::Bytes::new())
                }
            }
        });
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    for name in ["content-type", "cache-control"] {
        if let Some(value) = headers.get(name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}
struct ProxyTrace {
    studio: Arc<Studio>,
    tracker: inference::Tracker,
    json: bool,
    eof: bool,
}
impl ProxyTrace {
    fn feed(&mut self, bytes: &[u8]) {
        if self.json {
            self.tracker.feed_json(bytes);
        } else {
            self.tracker.feed(bytes);
        }
    }
    fn fail(&mut self) {
        self.tracker.info.metrics.status = "error".into();
    }
}
impl Drop for ProxyTrace {
    fn drop(&mut self) {
        // Non-inference proxy endpoints must not create misleading request rows.
        if self.tracker.info.metrics.source == "skip" {
            return;
        }
        self.tracker
            .finish(if self.eof { "error" } else { "cancelled" }, self.json);
        let s = self.studio.clone();
        let metrics = self.tracker.info.metrics.clone();
        tokio::spawn(async move {
            s.record_request(metrics).await;
        });
    }
}
// The guard closes intervals on success, request failure and client disconnect.
struct InferenceGuard(Arc<Studio>);
impl InferenceGuard {
    async fn new(s: Arc<Studio>) -> Self {
        s.active_requests.fetch_add(1, Ordering::SeqCst);
        s.memory_usage.lock().await.inference(true);
        Self(s)
    }
}
impl Drop for InferenceGuard {
    fn drop(&mut self) {
        let s = self.0.clone();
        s.active_requests.fetch_sub(1, Ordering::SeqCst);
        tokio::spawn(async move {
            s.memory_usage.lock().await.inference(false);
        });
    }
}
// The conversation and in-flight generation live here, rather than in a WebView.
async fn chat(State(s): State<Arc<Studio>>, h: HeaderMap, Json(mut body): Json<Value>) -> Response {
    if !authorized(&h, &s) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let user = body["messages"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(|v| serde_json::from_value::<Message>(v.clone()).ok());
    let user = match user {
        Some(m)
            if m.role == "user"
                && (!m.content.trim().is_empty() || !m.attachments.is_empty())
                && m.content.len() <= 1_000_000 =>
        {
            m
        }
        _ => return error(StatusCode::BAD_REQUEST, "Invalid user message"),
    };
    let lifecycle = s.lifecycle.lock().await;
    let (port, key) = {
        let mut p = s.process.lock().await;
        if !p
            .child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        {
            return error(StatusCode::SERVICE_UNAVAILABLE, "Engine is offline");
        }
        (p.port, p.key.clone())
    };
    let settings = s.settings.read().await;
    if settings.engine == engine::Engine::VllmApple {
        body["model"] = json!(if settings.vllm.backend == "mlx_lm" {
            "default_model"
        } else {
            &settings.vllm.model
        });
        body["stream_options"] = json!({"include_usage":true});
    }
    let is_omni = settings.engine == engine::Engine::Lfm3Omni;
    let omni_settings = settings.omni.clone();
    if is_omni {
        if s.active_requests.load(Ordering::SeqCst) != 0 {
            return error(
                StatusCode::CONFLICT,
                "Omni inference or media processing is busy",
            );
        }
        body["model"] = json!(omni::MODEL_ID);
        body["temperature"] = json!(0);
        body["stream_options"] = json!({"include_usage":true});
    }
    if !is_omni && !user.attachments.is_empty() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Media requires the qualified Omni engine",
        );
    }
    let engine_label = settings.engine.label().to_string();
    drop(settings);
    let mut tracker = inference::Tracker::new("chat", body["model"].as_str().unwrap_or(""));
    tracker.info.metrics.engine = Some(engine_label);
    {
        let mut d = s.document.write().await;
        if body["conversationId"]
            .as_str()
            .is_some_and(|id| id != d.conversations.active_id)
        {
            return error(
                StatusCode::CONFLICT,
                "Conversation changed; refresh before sending",
            );
        }
        if d.generating {
            return error(StatusCode::CONFLICT, "A conversation is already generating");
        }
        if d.messages.len() >= 1000 {
            return error(StatusCode::BAD_REQUEST, "Start a new conversation");
        }
        let mut pending = d
            .messages
            .iter()
            .filter(|m| m.role != "assistant" || !m.content.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        pending.push(user.clone());
        if is_omni {
            let messages = match omni_runtime::messages(&pending, &omni_settings) {
                Ok(v) => v,
                Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, e),
            };
            body["messages"] = json!(messages);
            if let Err(e) = s.check_omni(port, &key, &body, omni_settings.context).await {
                return error(e.status(), e);
            }
        } else {
            if pending.iter().any(|m| !m.attachments.is_empty()) {
                return error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Conversation contains Omni media; use Omni or a new conversation",
                );
            }
            body["messages"] = json!(pending
                .iter()
                .map(|m| json!({"role":m.role,"content":m.content}))
                .collect::<Vec<_>>());
            if let Err(rejection) = s
                .token_counter
                .lock()
                .await
                .check(&s.settings.read().await.clone(), &body)
                .await
            {
                return rejection.response();
            }
        }
        d.generating = true;
        d.conversations.touch();
        let mut user = user;
        user.inference = None;
        d.messages.push(user);
        body["stream"] = json!(true);
        d.messages.push(Message {
            attachments: Vec::new(),
            role: "assistant".into(),
            content: String::new(),
            inference: Some(tracker.info.clone()),
        });
    }
    body.as_object_mut().unwrap().remove("conversationId");
    let inference = InferenceGuard::new(s.clone()).await;
    drop(lifecycle);
    let response = s
        .client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await;
    let mut response = match response {
        Ok(r) if r.status().is_success() => r,
        other => {
            {
                let mut d = s.document.write().await;
                d.generating = false;
                tracker.finish("error", false);
                if let Some(last) = d.messages.last_mut() {
                    last.inference = Some(tracker.info.clone());
                }
            }
            s.record_request(tracker.info.metrics.clone()).await;
            let _ = s.persist().await;
            return match other {
                Ok(r) => {
                    let status = r.status();
                    let bytes = r.bytes().await.unwrap_or_default();
                    (status, bytes).into_response()
                }
                Err(e) => error(StatusCode::BAD_GATEWAY, e),
            };
        }
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(8);
    tokio::spawn(async move {
        let _inference = inference;
        let mut fallback = "error";
        loop {
            let chunk = tokio::select! {_ = tx.closed()=>{fallback="cancelled";break},chunk=response.chunk()=>chunk};
            match chunk {
                Ok(Some(bytes)) => {
                    tracker.feed(&bytes);
                    {
                        let mut d = s.document.write().await;
                        if let Some(last) = d.messages.last_mut() {
                            last.content = tracker.answer.clone();
                            last.inference = Some(tracker.info.clone());
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        fallback = "cancelled";
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e))).await;
                    break;
                }
            }
        }
        tracker.finish(fallback, false);
        {
            let mut d = s.document.write().await;
            d.generating = false;
            d.conversations.touch();
            if let Some(last) = d.messages.last_mut() {
                last.content = tracker.answer.clone();
                last.inference = Some(tracker.info.clone());
            }
        }
        s.record_request(tracker.info.metrics.clone()).await;
        if let Err(e) = s.persist().await {
            push_log(&s.logs, format!("[studio] Save failed: {e}")).await;
        }
    });
    let mut r = Response::new(Body::from_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    ));
    r.headers_mut()
        .insert("content-type", "text/event-stream".parse().unwrap());
    r.headers_mut()
        .insert("cache-control", "no-cache".parse().unwrap());
    r
}
async fn omni_count(
    State(studio): State<Arc<Studio>>,
    h: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    if !authorized(&h, &studio) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let _lifecycle = studio.lifecycle.lock().await;
    let settings = studio.settings.read().await.clone();
    if settings.engine != engine::Engine::Lfm3Omni
        || studio.active_requests.load(Ordering::SeqCst) != 0
    {
        return error(StatusCode::CONFLICT, "Select an idle qualified Omni engine");
    }
    let (port, key) = {
        let mut p = studio.process.lock().await;
        if !p
            .child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        {
            return error(StatusCode::SERVICE_UNAVAILABLE, "Engine is offline");
        }
        (p.port, p.key.clone())
    };
    let user: Message = match body["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| serde_json::from_value::<Message>(m.clone()).ok())
    {
        Some(m) if m.role == "user" => m,
        _ => return error(StatusCode::BAD_REQUEST, "Invalid Omni user message"),
    };
    let d = studio.document.read().await;
    if d.generating
        || body["conversationId"]
            .as_str()
            .is_some_and(|id| id != d.conversations.active_id)
    {
        return error(StatusCode::CONFLICT, "Conversation changed or generating");
    }
    let mut pending = d
        .messages
        .iter()
        .filter(|m| m.role != "assistant" || !m.content.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    pending.push(user);
    body["messages"] = match omni_runtime::messages(&pending, &settings.omni) {
        Ok(m) => json!(m),
        Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    body["model"] = json!(omni::MODEL_ID);
    body["temperature"] = json!(0);
    match studio
        .check_omni(port, &key, &body, settings.omni.context)
        .await
    {
        Ok(tokens) => {
            Json(json!({"inputTokens":tokens,"context":settings.omni.context,"exact":true}))
                .into_response()
        }
        Err(e) => error(e.status(), e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OmniImport {
    path: String,
    kind: omni::InputMode,
}
async fn omni_import(
    State(studio): State<Arc<Studio>>,
    h: HeaderMap,
    Json(input): Json<OmniImport>,
) -> Response {
    if !authorized(&h, &studio) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    let root = studio.settings.read().await.omni.media_root.clone();
    let _assets = studio.omni_assets.lock().await;
    match tokio::task::spawn_blocking(move || {
        omni_runtime::import_asset(&root, &input.path, input.kind)
    })
    .await
    {
        Ok(Ok(asset)) => Json(json!(asset)).into_response(),
        Ok(Err(e)) => error(StatusCode::UNPROCESSABLE_ENTITY, e),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
async fn omni_capabilities(State(studio): State<Arc<Studio>>, h: HeaderMap) -> Response {
    if !authorized(&h, &studio) {
        return error(StatusCode::UNAUTHORIZED, "Studio token required");
    }
    Json(omni::capabilities()).into_response()
}
pub fn router(s: Arc<Studio>) -> Router {
    Router::new()
        .route("/studio/session", post(session))
        .route("/studio/state", get(snapshot))
        .route("/studio/inspect", get(inspect))
        .route("/studio/models", get(catalog::scan_handler))
        .route("/studio/omni/capabilities", get(omni_capabilities))
        .route("/studio/omni/assets/import", post(omni_import))
        .route("/studio/omni/count", post(omni_count))
        .route("/studio/action", post(action))
        .route("/studio/chat", post(chat))
        .route(
            "/health",
            get(|| async { Json(json!({"service":"soliton","status":"ok"})) }),
        )
        .route("/v1/{*path}", axum::routing::any(proxy))
        .route("/status", get(proxy))
        .fallback_service(ServeDir::new("dist").append_index_html_on_directories(true))
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://tauri.localhost".parse().unwrap(),
                    "https://tauri.localhost".parse().unwrap(),
                    "tauri://localhost".parse().unwrap(),
                    "http://127.0.0.1:5180".parse().unwrap(),
                ])
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("x-studio-token"),
                ]),
        )
        .with_state(s)
}
pub async fn serve(
    listener: tokio::net::TcpListener,
    s: Arc<Studio>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let monitor = async {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let pid = s.engine_pid().await;
            s.memory_usage.lock().await.refresh(s.memory_bytes, pid);
        }
    };
    tokio::select! {
        result = axum::serve(listener, router(s.clone())).with_graceful_shutdown(shutdown) => result,
        _ = monitor => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn request_log_is_bounded_and_contains_no_response_text() {
        let s = Studio::new(PathBuf::new(), 8080);
        for i in 0..101 {
            let mut t = inference::Tracker::new("api", "test");
            t.info.metrics.id = i.to_string();
            s.record_request(t.info.metrics).await;
        }
        let log = s.requests.lock().await;
        assert_eq!(log.len(), 100);
        assert_eq!(log.front().unwrap().id, "1");
        assert_eq!(log.back().unwrap().id, "100");
        let v = serde_json::to_value(log.back()).unwrap();
        assert!(v.get("reasoning").is_none());
    }
    #[tokio::test]
    async fn appearance_changes_do_not_touch_engine_or_generation() {
        let s = Arc::new(Studio::new(PathBuf::new(), 8080));
        s.document.write().await.generating = true;
        let before = s.settings.read().await.metal_memory.clone();
        let mut h = HeaderMap::new();
        h.insert("x-studio-token", s.token.parse().unwrap());
        let r = action(
            State(s.clone()),
            h.clone(),
            Json(json!({"action":"appearance","theme":"light","chatFontSize":24})),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(s.document.read().await.generating);
        assert_eq!(s.settings.read().await.metal_memory, before);
        assert_eq!(s.settings.read().await.chat_font_size, 24);
        let r = action(
            State(s.clone()),
            h,
            Json(json!({"action":"appearance","theme":"invalid","chatFontSize":12})),
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(s.settings.read().await.chat_font_size, 24);
    }
    #[test]
    fn appearance_defaults_migrate_and_validate() {
        let mut legacy = serde_json::to_value(Settings::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("chatFontSize");
        legacy.as_object_mut().unwrap().remove("theme");
        let restored: Settings = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.chat_font_size, 13);
        assert_eq!(restored.theme, "dark");
        assert!(valid_appearance(12, "light"));
        assert!(valid_appearance(32, "dark"));
        assert!(!valid_appearance(11, "light"));
        assert!(!valid_appearance(33, "dark"));
        assert!(!valid_appearance(16, "invalid"));
    }
    #[tokio::test]
    async fn memory_restart_checks_before_stopping_or_saving() {
        let s = Studio::new(PathBuf::new(), 8080);
        let before = s.settings.read().await.metal_memory.clone();
        let mut changed = before.clone();
        changed.context = 0;
        assert!(s
            .apply_memory_restart(changed)
            .await
            .unwrap_err()
            .contains("Invalid"));
        s.document.write().await.generating = true;
        let mut changed = before.clone();
        changed.automatic = false;
        changed.context = 4096;
        assert!(s
            .apply_memory_restart(changed)
            .await
            .unwrap_err()
            .contains("reply"));
        assert_eq!(s.settings.read().await.metal_memory, before);
    }
    #[tokio::test]
    async fn failed_memory_restart_keeps_saved_values_and_reports_failure() {
        let path = std::env::temp_dir().join(format!("restart-{}.json", uuid::Uuid::new_v4()));
        let s = Studio::new(PathBuf::new(), 8080).with_storage(path.clone());
        let memory = metal_policy::MemorySettings {
            automatic: false,
            context: 4096,
            ..Default::default()
        };
        let error = s.apply_memory_restart(memory.clone()).await.unwrap_err();
        assert!(error.contains("saved") && error.contains("restart failed"));
        assert_eq!(
            Studio::new(PathBuf::new(), 8080)
                .with_storage(path.clone())
                .settings
                .read()
                .await
                .metal_memory,
            memory
        );
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn refuses_missing_model_config() {
        let s = Studio::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Strata"),
            8080,
        );
        s.settings.write().await.config.clear();
        assert!(s.start().await.unwrap_err().contains("config"));
    }
    #[tokio::test]
    async fn docking_actions_preserve_messages_and_guard_native_group_membership() {
        let studio = Arc::new(Studio::new(PathBuf::new(), 8081));
        let mut headers = HeaderMap::new();
        headers.insert("x-studio-token", studio.token.parse().unwrap());
        studio.document.write().await.messages.push(Message {
            attachments: Vec::new(),
            role: "user".into(),
            content: "keep".into(),
            inference: None,
        });
        let apply = |value| action(State(studio.clone()), headers.clone(), Json(value));
        assert_eq!(apply(json!({"action":"dockWorkspaceTab","tab":"logs","target":"chat","placement":"right"})).await.status(), StatusCode::OK);
        assert_eq!(studio.document.read().await.layout.groups().len(), 2);
        studio
            .document
            .write()
            .await
            .detached
            .push("group-logs".into());
        for value in [
            json!({"action":"resetWorkspace"}),
            json!({"action":"dockWorkspaceTab","tab":"logs","target":"chat","placement":"center"}),
        ] {
            assert_eq!(apply(value).await.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(
            apply(json!({"action":"resizeWorkspace","ratio":61}))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            apply(json!({"action":"workspaceTab","tab":"logs"}))
                .await
                .status(),
            StatusCode::OK
        );
        studio.document.write().await.detached.clear();
        assert_eq!(apply(json!({"action":"dockWorkspaceTab","tab":"logs","target":"chat","placement":"center"})).await.status(), StatusCode::OK);
        assert_eq!(
            apply(json!({"action":"resizeWorkspace","ratio":999}))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let d = studio.document.read().await;
        assert_eq!(d.messages.len(), 1);
        assert_eq!(d.messages[0].content, "keep");
        assert!(d.layout.workspace_split.is_none());
        assert_eq!(d.layout.groups()[0].tabs, ["chat", "logs"]);
    }
    #[test]
    fn authentication_requires_exact_token() {
        let s = Studio::new(PathBuf::new(), 8080);
        let mut h = HeaderMap::new();
        assert!(!authorized(&h, &s));
        h.insert("x-studio-token", s.token.parse().unwrap());
        assert!(authorized(&h, &s));
    }
    #[tokio::test]
    async fn saves_document_and_restores_without_stale_windows_or_generation() {
        let path = std::env::temp_dir().join(format!("studio-{}.json", uuid::Uuid::new_v4()));
        let s = Studio::new(PathBuf::new(), 8080).with_storage(path.clone());
        {
            let mut d = s.document.write().await;
            d.messages.push(Message {
                attachments: Vec::new(),
                role: "user".into(),
                content: "保存する会話".into(),
                inference: Some(inference::Tracker::new("chat", "test").info),
            });
            d.detached.push("api".into());
            d.generating = true;
            d.layout.engine_width = 340;
            d.layout.workspace("logs", Some("chat")).unwrap();
            d.layout.workspace("logs", None).unwrap();
            d.layout.dock("logs", "chat", "left").unwrap();
            d.layout.resize_workspace(60).unwrap();
        }
        s.persist().await.unwrap();
        let restored = Studio::new(PathBuf::new(), 8080).with_storage(path.clone());
        let d = restored.document.read().await;
        assert_eq!(d.messages[0].content, "保存する会話");
        assert_eq!(
            d.messages[0].inference.as_ref().unwrap().metrics.status,
            "cancelled"
        );
        assert!(d.messages[0]
            .inference
            .as_ref()
            .unwrap()
            .metrics
            .elapsed_ms
            .is_none());
        assert!(d.detached.is_empty());
        assert!(!d.generating);
        assert_eq!(d.layout.engine_width, 340);
        assert_eq!(d.layout.workspace_order, ["logs", "chat"]);
        assert_eq!(d.layout.workspace_active, "logs");
        assert_eq!(d.layout.workspace_split.as_deref(), Some("horizontal"));
        assert_eq!(d.layout.workspace_ratio, 60);
        assert_ne!(s.token, restored.token);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod engine_tests;
