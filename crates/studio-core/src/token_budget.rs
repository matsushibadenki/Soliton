//! Fail-closed text budget for qualified MLX requests when upstream /tokenize is absent.
use crate::{
    engine::{Engine, VllmSettings},
    Settings,
};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

#[derive(Debug)]
pub struct Rejection {
    pub code: &'static str,
    pub message: &'static str,
}
impl Rejection {
    pub fn response(&self) -> axum::response::Response {
        use axum::response::IntoResponse;
        (
            if self.code == "tokenization_unavailable" {
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            } else {
                axum::http::StatusCode::UNPROCESSABLE_ENTITY
            },
            axum::Json(json!({"error":{"code":self.code,"type":self.code,"message":self.message}})),
        )
            .into_response()
    }
}
fn invalid() -> Rejection {
    Rejection{code:"invalid_request_token_budget", message:"Qualified MLX inference requires bounded plain text and an explicit positive max_tokens"}
}
fn unavailable() -> Rejection {
    Rejection {
        code: "tokenization_unavailable",
        message: "Local token counting failed; inference was not started. Stop and restart the engine before retrying",
    }
}
fn request_budget(body: &Value) -> Result<u64, Rejection> {
    let output = body["max_tokens"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    if body["max_completion_tokens"]
        .as_u64()
        .is_some_and(|v| v != output)
        || body
            .get("max_completion_tokens")
            .is_some_and(|v| !v.is_u64())
        || body.get("n").is_some_and(|v| v != 1)
        || [
            "tools",
            "functions",
            "tool_choice",
            "chat_template",
            "add_generation_prompt",
            "continue_final_message",
            "add_special_tokens",
            "role_mapping",
            "images",
        ]
        .iter()
        .any(|k| body.get(*k).is_some())
    {
        return Err(invalid());
    }
    if let Some(kwargs) = body.get("chat_template_kwargs") {
        let Some(map) = kwargs.as_object() else {
            return Err(invalid());
        };
        if map
            .iter()
            .any(|(k, v)| k != "enable_thinking" || !v.is_boolean())
        {
            return Err(invalid());
        }
    }
    if let Some(messages) = body.get("messages") {
        let Some(messages) = messages
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 1000)
        else {
            return Err(invalid());
        };
        if messages.iter().any(|m| {
            m.as_object()
                .is_none_or(|o| o.keys().any(|k| k != "role" && k != "content"))
                || !matches!(m["role"].as_str(), Some("system" | "user" | "assistant"))
                || !m["content"].is_string()
        }) {
            return Err(invalid());
        }
    } else if !body["prompt"].is_string() {
        return Err(invalid());
    }
    Ok(output)
}
fn within_limit(prompt: u64, output: u64, context: u32) -> Result<(), Rejection> {
    if prompt
        .checked_add(output)
        .is_none_or(|n| n > u64::from(context))
    {
        Err(Rejection{code:"context_length_exceeded",message:"Prompt plus requested output exceeds the qualified context; shorten the conversation or start a new one"})
    } else {
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Counter {
    worker: Option<Worker>,
    invalidated: bool,
}
struct Worker {
    settings: VllmSettings,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Covers cancellation mid-frame and application teardown as well as failures.
        if matches!(self.child.try_wait(), Ok(None)) {
            #[cfg(unix)]
            if let Some(id) = self.child.id() {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.start_kill();
        }
    }
}
impl Worker {
    fn spawn(settings: &VllmSettings) -> Result<Self, Rejection> {
        let python = Path::new(&settings.executable)
            .parent()
            .ok_or_else(unavailable)?
            .join("python");
        let mut command = Command::new(python);
        command
            .args([
                "-I",
                "-u",
                "-c",
                include_str!("token_budget.py"),
                &settings.model,
                "--soliton-token-counter",
            ])
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| unavailable())?;
        Ok(Self {
            settings: settings.clone(),
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 0,
        })
    }
    async fn exchange(&mut self, body: &Value) -> Result<u64, Rejection> {
        self.next_id = self.next_id.checked_add(1).ok_or_else(unavailable)?;
        let mut input = serde_json::to_vec(&json!({"id":self.next_id,"request":body}))
            .map_err(|_| invalid())?;
        input.push(b'\n');
        if input.len() > 2 * 1024 * 1024 {
            return Err(invalid());
        }
        self.stdin
            .write_all(&input)
            .await
            .map_err(|_| unavailable())?;
        self.stdin.flush().await.map_err(|_| unavailable())?;
        let mut out = Vec::new();
        loop {
            let buffer = self.stdout.fill_buf().await.map_err(|_| unavailable())?;
            if buffer.is_empty() {
                return Err(unavailable());
            }
            let newline = buffer.iter().position(|b| *b == b'\n');
            let n = newline.map_or(buffer.len(), |i| i + 1);
            if out.len() + n > 4096 {
                return Err(unavailable());
            }
            out.extend_from_slice(&buffer[..n]);
            self.stdout.consume(n);
            if newline.is_some() {
                break;
            }
        }
        let result: Value = serde_json::from_slice(&out).map_err(|_| unavailable())?;
        if result["id"].as_u64() != Some(self.next_id) {
            return Err(unavailable());
        }
        result["prompt_tokens"].as_u64().ok_or_else(unavailable)
    }
    async fn stop(&mut self) -> Result<(), String> {
        if matches!(self.child.try_wait(), Ok(None)) {
            #[cfg(unix)]
            if let Some(id) = self.child.id() {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
            self.child
                .start_kill()
                .map_err(|_| "Cannot stop token counter")?;
        }
        tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .map_err(|_| "Token counter did not exit")?
            .map_err(|_| "Cannot reap token counter")?;
        Ok(())
    }
}
impl Counter {
    pub async fn stop(&mut self) -> Result<(), String> {
        if let Some(mut worker) = self.worker.take() {
            if let Err(e) = worker.stop().await {
                self.worker = Some(worker);
                return Err(e);
            }
        }
        self.invalidated = false;
        Ok(())
    }
    // Called only after the owned daemon exposes control_ready (startup gates passed).
    // Spawn is nonblocking: Python loads its tokenizer before reading the first frame.
    // Never send a synthetic prompt, consume protocol bytes or revive a failed worker.
    pub fn prepare(&mut self, settings: &Settings) -> Result<bool, Rejection> {
        if settings.engine != Engine::VllmApple
            || settings.vllm.backend != "mlx_lm"
            || settings.vllm.evidence.is_empty()
        {
            return Ok(false);
        }
        if self.invalidated {
            return Err(unavailable());
        }
        if let Some(worker) = self.worker.as_mut() {
            if worker.settings != settings.vllm || !matches!(worker.child.try_wait(), Ok(None)) {
                self.invalidated = true;
                return Err(unavailable());
            }
            return Ok(false);
        }
        match Worker::spawn(&settings.vllm) {
            Ok(worker) => {
                self.worker = Some(worker);
                Ok(true)
            }
            Err(error) => {
                self.invalidated = true;
                Err(error)
            }
        }
    }
    // Caller holds the Studio lifecycle lock. Taking the worker out of its slot
    // makes a cancelled request destroy it rather than reuse a partial frame.
    pub async fn check(&mut self, settings: &Settings, body: &Value) -> Result<(), Rejection> {
        if settings.engine != Engine::VllmApple
            || settings.vllm.backend != "mlx_lm"
            || settings.vllm.evidence.is_empty()
        {
            self.stop().await.map_err(|_| unavailable())?;
            return Ok(());
        }
        let output = request_budget(body)?;
        within_limit(0, output, settings.vllm.context)?;
        if self.invalidated {
            return Err(unavailable());
        }
        if serde_json::to_vec(body).map_err(|_| invalid())?.len() + 64 > 2 * 1024 * 1024 {
            return Err(invalid());
        }
        self.invalidated = true;
        let mut worker = match self.worker.take() {
            Some(w) if w.settings == settings.vllm => w,
            Some(mut w) => {
                w.stop().await.map_err(|_| unavailable())?;
                Worker::spawn(&settings.vllm)?
            }
            None => Worker::spawn(&settings.vllm)?,
        };
        let result = tokio::time::timeout(Duration::from_secs(30), worker.exchange(body)).await;
        let count = match result {
            Ok(Ok(count)) => count,
            _ => {
                let _ = worker.stop().await;
                return Err(unavailable());
            }
        };
        self.worker = Some(worker);
        self.invalidated = false;
        within_limit(count, output, settings.vllm.context)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn preparation_never_counts_or_restarts_a_failed_worker() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("soliton-prewarm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let python = dir.join("python");
        std::fs::write(&python, "#!/usr/bin/python3\nimport sys,json\nfor line in sys.stdin:\n v=json.loads(line)\n assert v['id']==1\n print(json.dumps({'id':v['id'],'prompt_tokens':10}),flush=True)\n").unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut settings = Settings::default();
        let mut counter = Counter::default();
        assert!(!counter.prepare(&settings).unwrap());
        assert!(counter.worker.is_none());
        settings.engine = Engine::VllmApple;
        settings.vllm.backend = "mlx_lm".into();
        settings.vllm.evidence = "/fixture/qualified".into();
        settings.vllm.executable = dir.join("server").to_string_lossy().into();
        assert!(counter.prepare(&settings).unwrap());
        let pid = counter.worker.as_ref().unwrap().child.id().unwrap();
        assert_eq!(counter.worker.as_ref().unwrap().next_id, 0);
        assert!(!counter.prepare(&settings).unwrap());
        counter
            .check(&settings, &json!({"prompt":"hello","max_tokens":16}))
            .await
            .unwrap();
        assert_eq!(counter.worker.as_ref().unwrap().child.id(), Some(pid));
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        counter.worker.as_mut().unwrap().child.wait().await.unwrap();
        assert!(counter.prepare(&settings).is_err());
        assert!(counter.prepare(&settings).is_err());
        assert_eq!(
            counter
                .check(&settings, &json!({"prompt":"hello","max_tokens":16}))
                .await
                .unwrap_err()
                .code,
            "tokenization_unavailable"
        );
        counter.stop().await.unwrap();
        assert!(counter.prepare(&settings).unwrap());
        counter.stop().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn reuse_failure_and_stop_are_isolated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("soliton-counter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let python = dir.join("python");
        std::fs::write(&python,"#!/usr/bin/python3\nimport sys,json\nfor line in sys.stdin:\n v=json.loads(line)\n print(json.dumps({'id':v['id'],'prompt_tokens':10}),flush=True)\n").unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut settings = Settings::default();
        settings.engine = Engine::VllmApple;
        settings.vllm.evidence = "/fixture/evidence".into();
        settings.vllm.executable = dir.join("server").to_string_lossy().into();
        let request = json!({"messages":[{"role":"user","content":"hello"}],"max_tokens":64});
        let mut counter = Counter::default();
        counter.check(&settings, &request).await.unwrap();
        let pid = counter.worker.as_ref().unwrap().child.id().unwrap();
        counter.check(&settings, &request).await.unwrap();
        assert_eq!(counter.worker.as_ref().unwrap().child.id(), Some(pid));
        let mut overflow = request.clone();
        overflow["max_tokens"] = json!(1024);
        assert_eq!(
            counter.check(&settings, &overflow).await.unwrap_err().code,
            "context_length_exceeded"
        );
        counter.check(&settings, &request).await.unwrap();
        assert_eq!(counter.worker.as_ref().unwrap().child.id(), Some(pid));
        let mut changed = settings.clone();
        changed.vllm.evidence = "/fixture/new-evidence".into();
        counter.check(&changed, &request).await.unwrap();
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        counter.stop().await.unwrap();
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        std::fs::write(&python,"#!/usr/bin/python3\nimport sys\nfor line in sys.stdin: print('PRIVATE invalid json',flush=True)\n").unwrap();
        assert_eq!(
            counter.check(&settings, &request).await.unwrap_err().code,
            "tokenization_unavailable"
        );
        assert!(counter.worker.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_bad_frames_and_dead_workers_require_restart() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("soliton-counter-faults-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let python = dir.join("python");
        let good="#!/usr/bin/python3\nimport sys,json,time\nfor line in sys.stdin:\n v=json.loads(line)\n if v['id']>1: time.sleep(10)\n print(json.dumps({'id':v['id'],'prompt_tokens':10}),flush=True)\n";
        std::fs::write(&python, good).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut settings = Settings::default();
        settings.engine = Engine::VllmApple;
        settings.vllm.evidence = "/fixture/evidence".into();
        settings.vllm.executable = dir.join("server").to_string_lossy().into();
        let request = json!({"messages":[{"role":"user","content":"hello"}],"max_tokens":64});
        let mut c = Counter::default();
        c.check(&settings, &request).await.unwrap();
        let pid = c.worker.as_ref().unwrap().child.id().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), c.check(&settings, &request))
                .await
                .is_err()
        );
        assert!(c.worker.is_none());
        assert!(c.invalidated);
        for _ in 0..100 {
            if unsafe { libc::kill(pid as i32, 0) } == -1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert!(c.check(&settings, &request).await.is_err());
        c.stop().await.unwrap();
        c.check(&settings, &request).await.unwrap();
        let pid = c.worker.as_ref().unwrap().child.id().unwrap();
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        assert!(c.check(&settings, &request).await.is_err());
        assert!(c.invalidated);
        c.stop().await.unwrap();
        for output in [
            "{\"id\":999,\"prompt_tokens\":10}",
            "x".repeat(5000).as_str(),
        ] {
            std::fs::write(&python,format!("#!/usr/bin/python3\nimport sys\nfor line in sys.stdin: print({:?},flush=True)\n",output)).unwrap();
            assert!(c.check(&settings, &request).await.is_err());
            assert!(c.worker.is_none());
            c.stop().await.unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn missing_counter_fails_closed_without_launching_inference() {
        let mut settings = Settings::default();
        settings.engine = Engine::VllmApple;
        settings.vllm.evidence = "/fixture/evidence".into();
        settings.vllm.executable = "/nonexistent-soliton-backend/server".into();
        let request = json!({"messages":[{"role":"user","content":"hello"}],"max_tokens":64});
        assert_eq!(
            Counter::default()
                .check(&settings, &request)
                .await
                .unwrap_err()
                .code,
            "tokenization_unavailable"
        );
    }
    #[test]
    fn budget_covers_templates_output_and_unsupported_shapes() {
        let plain = json!({"messages":[{"role":"user","content":"hello"}],"max_tokens":64});
        assert_eq!(request_budget(&plain).unwrap(), 64);
        assert!(within_limit(960, 64, 1024).is_ok());
        assert_eq!(
            within_limit(961, 64, 1024).unwrap_err().code,
            "context_length_exceeded"
        );
        assert!(within_limit(u64::MAX, 64, 1024).is_err());
        for patch in [
            json!({"max_tokens":0}),
            json!({"max_completion_tokens":65}),
            json!({"n":2}),
            json!({"tools":[]}),
            json!({"chat_template_kwargs":{"anything":true}}),
            json!({"messages":[{"role":"user","content":[]} ]}),
        ] {
            let mut v = plain.clone();
            for (k, value) in patch.as_object().unwrap() {
                v[k] = value.clone();
            }
            assert!(request_budget(&v).is_err());
        }
    }
}
