use super::*;
use engine::{Engine, VllmSettings};

fn studio() -> Arc<Studio> {
    Arc::new(Studio::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Strata"),
        0,
    ))
}
#[tokio::test]
async fn switching_preserves_history_metrics_and_rejects_active_requests() {
    let s = studio();
    s.document.write().await.messages.push(Message {
        attachments: Vec::new(),
        role: "user".into(),
        content: "Keep this conversation".into(),
        ..Default::default()
    });
    s.record_request(
        inference::Tracker::new("chat", "previous-model")
            .info
            .metrics,
    )
    .await;
    let guard = InferenceGuard::new(s.clone()).await;
    assert!(s.switch_engine(Engine::VllmApple).await.is_err());
    assert_eq!(s.settings.read().await.engine, Engine::Strata);
    drop(guard);
    s.switch_engine(Engine::VllmApple).await.unwrap();
    assert_eq!(
        s.document.read().await.messages[0].content,
        "Keep this conversation"
    );
    assert_eq!(s.requests.lock().await.len(), 1);
    assert!(s.process.lock().await.child.is_none());
    assert!(s.apply_memory_restart(Default::default()).await.is_err());
    s.switch_engine(Engine::Strata).await.unwrap();
    assert!(s
        .logs
        .lock()
        .await
        .iter()
        .any(|line| line.contains("vLLM-Apple")));
}
#[cfg(unix)]
#[tokio::test]
async fn switch_reaps_running_engine_before_selecting_next() {
    let s = studio();
    let mut child = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    {
        let mut p = s.process.lock().await;
        p.group = Some(pid);
        // A child ignoring normal termination exercises group cleanup.
        assert!(child.try_wait().unwrap().is_none());
        p.child = Some(child);
    }
    s.switch_engine(Engine::VllmApple).await.unwrap();
    assert!(s.process.lock().await.child.is_none());
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert_eq!(s.settings.read().await.engine, Engine::VllmApple);
}
#[tokio::test]
async fn legacy_storage_migrates_without_modifying_original() {
    let dir = std::env::temp_dir().join(format!("soliton-migration-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = dir.join("old.json");
    let target = dir.join("new.json");
    let old = studio();
    let bytes = serde_json::to_vec(
        &json!({"settings":*old.settings.read().await,"document":*old.document.read().await}),
    )
    .unwrap();
    std::fs::write(&legacy, &bytes).unwrap();
    let migrated =
        Studio::new(old.root.clone(), 0).with_legacy_storage(target.clone(), legacy.clone());
    migrated.settings.write().await.engine = Engine::VllmApple;
    migrated.persist().await.unwrap();
    assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
    let reloaded = Studio::new(old.root.clone(), 0).with_legacy_storage(target, legacy);
    assert_eq!(reloaded.settings.read().await.engine, Engine::VllmApple);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_vllm_configuration_never_launches_or_downloads() {
    let mut settings = VllmSettings::default();
    assert!(settings.command(10001, 10002, "private-key").is_err());
    settings.backend = "arbitrary".into();
    assert!(!settings.valid());
    assert!(serde_json::from_value::<Engine>(json!("other")).is_err());
    let legacy: Settings = serde_json::from_value(json!({"strataRoot":"/test", "python":"python3", "config":"", "mock":false,"temperature":0.7,"maxTokens":512,"language":"ja"})).unwrap();
    assert_eq!(legacy.engine, Engine::Strata);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn switch_cleans_separate_native_worker_after_parent_crash() {
    let s = studio();
    let mut child = Command::new("/usr/bin/python3")
        .args(["-u", "-c", "import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'],start_new_session=True);print(p.pid,flush=True);time.sleep(60)"])
        .stdout(std::process::Stdio::piped()).process_group(0).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let worker: i32 = lines.next_line().await.unwrap().unwrap().parse().unwrap();
    let parent = child.id().unwrap();
    {
        let mut p = s.process.lock().await;
        p.group = Some(parent);
        p.child = Some(child);
        p.memory = Some(json!({}));
    }
    s.engine_pid().await; // monitor captures native identity while parent is alive
    unsafe {
        libc::kill(parent as i32, libc::SIGKILL);
    }
    s.process
        .lock()
        .await
        .child
        .as_mut()
        .unwrap()
        .wait()
        .await
        .unwrap();
    s.switch_engine(Engine::VllmApple).await.unwrap();
    assert!(!owned_process::capture(worker as u32)
        .unwrap()
        .iter()
        .any(|p| p.alive()));
}

#[test]
fn evidence_launch_preserves_checks_and_sets_explicit_context() {
    let dir = std::env::temp_dir().join(format!("soliton-evidence-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("vllm_apple")).unwrap();
    std::fs::create_dir_all(dir.join("model")).unwrap();
    std::fs::write(dir.join("vllm_apple/cli.py"), "# fixture").unwrap();
    std::fs::write(dir.join("evidence.json"), "{}").unwrap();
    let mut settings = VllmSettings {
        root: dir.to_string_lossy().into(),
        python: "/usr/bin/python3".into(),
        model: dir.join("model").to_string_lossy().into(),
        executable: "/usr/bin/python3".into(),
        evidence: dir.join("evidence.json").to_string_lossy().into(),
        context: 1024,
        ..Default::default()
    };
    let command = settings
        .command(19201, 19202, "private-test-token")
        .unwrap();
    let args: Vec<_> = command
        .as_std()
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert!(args.windows(2).any(|a| a == ["--max-model-len", "1024"]));
    assert!(args
        .windows(2)
        .any(|a| a[0] == "--architecture-evidence" && a[1] == settings.evidence));
    assert!(args.iter().any(|a| a == "--disable-metal-tuning"));
    assert!(!args
        .iter()
        .any(|a| a.contains("private-test-token") || a.starts_with("--skip")));
    let inspection = settings.inspection_command().unwrap();
    let inspection_args: Vec<_> = inspection
        .as_std()
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert!(inspection_args.iter().any(|s| s == "inspect-model"));
    assert!(inspection_args
        .windows(2)
        .any(|a| a[0] == "--architecture-evidence" && a[1] == settings.evidence));
    assert!(inspection_args
        .windows(2)
        .any(|a| a == ["--max-model-len", "1024"]));
    assert!(!inspection_args
        .iter()
        .any(|s| s.starts_with("--skip") || s == "serve"));
    settings.kv_cache_mib = 1024;
    settings.allocator_cache_mib = 256;
    for cmd in [
        settings.command(19201, 19202, "token").unwrap(),
        settings.inspection_command().unwrap(),
    ] {
        let env = cmd
            .as_std()
            .get_envs()
            .find(|(k, _)| *k == "VLLM_APPLE_KV_CACHE_MIB")
            .unwrap();
        assert_eq!(env.1, Some(std::ffi::OsStr::new("1024")));
        assert_eq!(
            cmd.as_std()
                .get_envs()
                .find(|(k, _)| *k == "VLLM_APPLE_ALLOCATOR_CACHE_MIB")
                .unwrap()
                .1,
            Some(std::ffi::OsStr::new("256"))
        );
    }
    settings.kv_cache_mib = 0;
    settings.allocator_cache_mib = 0;
    assert_eq!(
        settings
            .command(19201, 19202, "token")
            .unwrap()
            .as_std()
            .get_envs()
            .find(|(k, _)| *k == "VLLM_APPLE_KV_CACHE_MIB")
            .unwrap()
            .1,
        None
    );
    assert_eq!(
        settings
            .inspection_command()
            .unwrap()
            .as_std()
            .get_envs()
            .find(|(k, _)| *k == "VLLM_APPLE_ALLOCATOR_CACHE_MIB")
            .unwrap()
            .1,
        None
    );
    settings.context = 0;
    assert!(!settings.valid());
    settings.context = 1024;
    settings.evidence = dir.to_string_lossy().into();
    assert!(settings.command(19201, 19202, "token").is_err());
    let legacy: VllmSettings = serde_json::from_value(json!({"root":"/local"})).unwrap();
    assert_eq!(legacy.context, 1024);
    assert!(legacy.evidence.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn kv_cache_policy_is_explicit_qualified_and_backward_compatible() {
    let mut s: VllmSettings = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(s.kv_cache_mib, 0);
    assert!(s.valid());
    s.kv_cache_mib = 1024;
    assert!(!s.valid(), "a bounded policy requires evidence");
    s.evidence = "/qualified.json".into();
    assert!(s.valid());
    assert_eq!(serde_json::to_value(&s).unwrap()["kvCacheMiB"], 1024);
    s.backend = "vllm_metal".into();
    assert!(!s.valid());
    s.backend = "mlx_lm".into();
    for value in [1, 63, 4097, u32::MAX] {
        s.kv_cache_mib = value;
        assert!(!s.valid());
    }
}

#[test]
fn allocator_policy_requires_qualified_kv_and_serializes_explicitly() {
    let mut s: VllmSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(s.allocator_cache_mib, 0);
    s.allocator_cache_mib = 256;
    assert!(!s.valid());
    s.evidence = "/qualified.json".into();
    assert!(!s.valid());
    s.kv_cache_mib = 1024;
    assert!(s.valid());
    assert_eq!(serde_json::to_value(&s).unwrap()["allocatorCacheMiB"], 256);
    s.backend = "vllm_metal".into();
    assert!(!s.valid());
    s.backend = "mlx_lm".into();
    for value in [1, 63, 4097, u32::MAX] {
        s.allocator_cache_mib = value;
        assert!(!s.valid());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn omni_switch_uses_shared_ownership_and_missing_evidence_never_spawns() {
    let s = studio();
    s.document.write().await.messages.push(Message {
        role: "user".into(),
        content: "preserve media history".into(),
        ..Default::default()
    });
    let child = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    {
        let mut p = s.process.lock().await;
        p.group = Some(pid);
        p.child = Some(child);
    }
    let active = InferenceGuard::new(s.clone()).await;
    assert!(s.switch_engine(Engine::Lfm3Omni).await.is_err());
    drop(active);
    s.switch_engine(Engine::Lfm3Omni).await.unwrap();
    assert!(s.process.lock().await.child.is_none());
    assert!(s.start().await.is_err());
    assert!(s.process.lock().await.child.is_none());
    assert_eq!(
        s.document.read().await.messages[0].content,
        "preserve media history"
    );
    s.switch_engine(Engine::Strata).await.unwrap();
}

#[tokio::test]
async fn omni_bad_count_latches_before_retrying_or_mutating_history() {
    use std::sync::atomic::AtomicUsize;
    let calls = Arc::new(AtomicUsize::new(0));
    let count_calls = calls.clone();
    let router = Router::new().route(
        "/v1/chat/completions/input_tokens",
        post(move || {
            let calls = count_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Json(json!({"input_tokens":true}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let s = studio();
    let body = json!({"messages":[{"role":"user","content":"fixture"}],"max_tokens":1});
    assert!(s.check_omni(port, "key", &body, 4096).await.is_err());
    assert!(s.omni_counter_failed.load(Ordering::SeqCst));
    assert!(s.check_omni(port, "key", &body, 4096).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(s.document.read().await.messages.is_empty());
    task.abort();
}

#[cfg(unix)]
fn prefix_fixture() -> (Arc<Studio>, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("soliton-prefix-host-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("runtime")).unwrap();
    std::fs::create_dir_all(dir.join("model")).unwrap();
    for name in [
        "prefix-cache-http.py",
        "prefix_cache_contract.py",
        "prefix_cache_roundtrip_support.py",
        "prefix_cache_writes.py",
    ] {
        std::fs::write(dir.join("runtime").join(name), "# fixture").unwrap();
    }
    std::fs::write(dir.join("evidence.json"), "{}").unwrap();
    let python = dir.join("python-fixture");
    std::fs::write(&python,"#!/bin/sh\ncase \"$*\" in *--verify-only*) printf '{\"verified\":true,\"binding_schema\":2,\"profile\":{\"schema\":2},\"cache_binding\":{\"binding_schema\":2},\"identity\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}\\n';; *) exec /bin/sleep 60;; esac\n").unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = studio();
    (s, dir)
}
#[cfg(unix)]
async fn set_prefix(s: &Studio, dir: &std::path::Path) {
    let mut settings = s.settings.write().await;
    settings.engine = Engine::MlxPrefix;
    settings.prefix_cache = prefix_engine::Settings {
        python: dir.join("python-fixture").to_string_lossy().into(),
        model: dir.join("model").to_string_lossy().into(),
        runtime_dir: dir.join("runtime").to_string_lossy().into(),
        store_executable: "/usr/bin/true".into(),
        cache_root: dir.join("cache").to_string_lossy().into(),
        quota_mib: 256,
        evidence: dir.join("evidence.json").to_string_lossy().into(),
    };
}
#[cfg(unix)]
#[tokio::test]
async fn prefix_lifecycle_is_exclusive_private_and_settings_are_locked() {
    let (s, dir) = prefix_fixture();
    set_prefix(&s, &dir).await;
    s.document.write().await.messages.push(Message {
        role: "user".into(),
        content: "Preserve me".into(),
        ..Default::default()
    });
    let config = s.settings.read().await.prefix_cache.clone();
    let (command, _) = config
        .command(18084, "secret-not-an-argument")
        .await
        .unwrap();
    let arguments = command
        .as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(!arguments
        .iter()
        .any(|a| a.contains("secret-not-an-argument") || a == "--qualification-mode"));
    assert!(arguments.iter().any(|a| a == "--evidence"));
    s.start().await.unwrap();
    assert!(s.start().await.is_err());
    let pid = s.process.lock().await.child.as_ref().unwrap().id().unwrap();
    let mut changed = s.settings.read().await.clone();
    changed.prefix_cache.quota_mib = 512;
    let mut headers = HeaderMap::new();
    headers.insert("x-studio-token", s.token.parse().unwrap());
    let result = action(
        State(s.clone()),
        headers,
        Json(json!({"action":"settings","settings":changed})),
    )
    .await;
    assert_eq!(result.status(), StatusCode::CONFLICT);
    assert_eq!(s.settings.read().await.prefix_cache.quota_mib, 256);
    s.switch_engine(Engine::Strata).await.unwrap();
    assert!(s.process.lock().await.child.is_none());
    assert!(!owned_process::capture(pid)
        .unwrap()
        .iter()
        .any(|p| p.alive()));
    assert_eq!(s.document.read().await.messages[0].content, "Preserve me");
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn prefix_missing_evidence_cannot_spawn_and_failed_counts_latch() {
    let (s, dir) = prefix_fixture();
    set_prefix(&s, &dir).await;
    std::fs::remove_file(dir.join("evidence.json")).unwrap();
    assert!(s.start().await.is_err());
    assert!(s.process.lock().await.child.is_none());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new().route(
        "/tokenize",
        post(|| async { Json(json!({"identity":"wrong","count":10,"context":512})) }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    s.process.lock().await.memory = Some(json!({"prefixIdentity":"expected"}));
    let body = json!({"model":"default_model","messages":[{"role":"user","content":"hello"}],"max_tokens":16});
    assert!(s.check_prefix(port, "key", &body).await.is_err());
    assert!(s.prefix_counter_failed.load(Ordering::SeqCst));
    assert!(s.check_prefix(1, "key", &body).await.is_err());
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn prefix_capacity_requires_auth_and_stopped_engine_without_changing_history() {
    use std::os::unix::fs::PermissionsExt;
    let (s, dir) = prefix_fixture();
    set_prefix(&s, &dir).await;
    assert_eq!(
        prefix_capacity(State(s.clone()), HeaderMap::new())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let missing = s.settings.read().await.prefix_cache.storage(false).unwrap();
    assert_eq!(missing["exists"], false);
    assert!(!dir.join("cache").exists());
    std::fs::create_dir(dir.join("cache")).unwrap();
    std::fs::set_permissions(dir.join("cache"), std::fs::Permissions::from_mode(0o700)).unwrap();
    // Interrupted worker staging is outside the store quota and remains visible even before store creation.
    let stage = dir.join("cache/stage-abcdefgh");
    std::fs::create_dir(&stage).unwrap();
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(stage.join("state.safetensors"), b"staged state").unwrap();
    let report = s.settings.read().await.prefix_cache.storage(false).unwrap();
    assert_eq!(report["exists"], false);
    assert_eq!(report["staging"]["bytes"], 12);
    assert_eq!(report["staging"]["directories"], 1);
    assert_eq!(report["staging"]["staleDirectories"], 0);
    let cleanable = dir.join("cache/stage-ijklmnop");
    std::fs::create_dir(&cleanable).unwrap();
    std::fs::set_permissions(&cleanable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let identity = prefix_store::Identity {
        model: "a".repeat(64),
        tokenizer: "b".repeat(64),
        runtime: "c".repeat(64),
        prefix: "d".repeat(64),
        format: "mlx-lm/0.32.0/gemma2-2b/http-prefix-v1".into(),
    };
    std::fs::write(
        cleanable.join("identity.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    std::fs::write(cleanable.join("state.safetensors"), "old state").unwrap();
    for path in [
        cleanable.join("identity.json"),
        cleanable.join("state.safetensors"),
        cleanable.clone(),
    ] {
        std::fs::File::open(path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
    }
    let store = prefix_store::Store::open(&dir.join("cache/store"), 256 * 1024 * 1024).unwrap();
    let orphan = dir
        .join("cache/store")
        .join(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&orphan, b"temporary").unwrap();
    std::fs::set_permissions(&orphan, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::File::open(&orphan)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
        .unwrap();
    s.document.write().await.messages.push(Message {
        role: "user".into(),
        content: "Keep conversation".into(),
        ..Default::default()
    });
    let settings_before = serde_json::to_value(&*s.settings.read().await).unwrap();
    s.start().await.unwrap();
    assert_eq!(
        maintain_prefix(&s, true).await.status(),
        StatusCode::CONFLICT
    );
    assert!(orphan.exists());
    assert_eq!(
        maintain_prefix_operation(&s, false, true).await.status(),
        StatusCode::CONFLICT
    );
    assert!(cleanable.exists());
    s.stop().await.unwrap();
    s.active_requests.store(1, Ordering::SeqCst);
    assert_eq!(
        maintain_prefix_operation(&s, false, true).await.status(),
        StatusCode::CONFLICT
    );
    s.active_requests.store(0, Ordering::SeqCst);
    s.document.write().await.generating = true;
    assert_eq!(
        maintain_prefix_operation(&s, false, true).await.status(),
        StatusCode::CONFLICT
    );
    s.document.write().await.generating = false;
    assert_eq!(
        maintain_prefix_operation(&s, false, true).await.status(),
        StatusCode::OK
    );
    assert!(!cleanable.exists());
    assert!(orphan.exists()); // Cleanup does not trim committed-store temporary files.
    assert_eq!(maintain_prefix(&s, true).await.status(), StatusCode::OK);
    assert!(!orphan.exists());
    assert_eq!(store.capacity().unwrap().used_bytes, 0);
    assert_eq!(
        std::fs::read(stage.join("state.safetensors")).unwrap(),
        b"staged state"
    );
    let report = s.settings.read().await.prefix_cache.storage(false).unwrap();
    assert_eq!(report["staging"]["bytes"], 12);
    assert_eq!(report["capacity"]["usedBytes"], 0);
    assert_eq!(
        s.document.read().await.messages[0].content,
        "Keep conversation"
    );
    assert_eq!(
        serde_json::to_value(&*s.settings.read().await).unwrap(),
        settings_before
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn gateway_records_admission_for_api_and_chat_without_changing_refused_history() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router=Router::new().route("/tokenize",post(||async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Json(json!({"identity":"a".repeat(64),"count":10,"context":512}))
    })).route("/v1/chat/completions",post(|Json(_value):Json<Value>|async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let value=json!({"choices":[{"message":{"content":"fixture"},"delta":{"content":"fixture"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":1,"total_tokens":11}});
        // This fixture intentionally exercises the same content in JSON and SSE.
        ([("content-type","text/event-stream")],format!("data: {}\n\ndata: [DONE]\n\n",value)).into_response()
    }));
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let s = studio();
    s.settings.write().await.engine = Engine::MlxPrefix;
    let mut child = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    assert!(child.try_wait().unwrap().is_none());
    {
        let mut p = s.process.lock().await;
        p.child = Some(child);
        p.group = Some(pid);
        p.port = port;
        p.key = "private-worker-secret".into();
        p.metrics_session = Some("public-session".into());
        p.memory = Some(json!({"prefixIdentity":"a".repeat(64)}));
    }
    let body = json!({"model":"default_model","messages":[{"role":"user","content":"fixture"}],"max_tokens":16,"stream":true});
    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .header("x-studio-token", &s.token)
        .body(Body::from(body.to_string()))
        .unwrap();
    let held = s.lifecycle.lock().await;
    let release = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(held);
    };
    let (response, ()) = tokio::join!(proxy(State(s.clone()), request), release);
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    for _ in 0..100 {
        if s.requests.lock().await.len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    {
        let rows = s.requests.lock().await;
        assert_eq!(rows.len(), 1);
        let m = &rows[0];
        let g = m.gateway.as_ref().unwrap();
        assert_eq!(m.status, "completed");
        assert_eq!(g.engine_session.as_deref(), Some("public-session"));
        assert!(g.admission_ms.unwrap() >= 20.0);
        assert!(g.first_content_ms.unwrap() >= g.dispatch_ms.unwrap());
        assert!(g.upstream_headers_ms.unwrap() >= 10.0);
        assert!(g.lifecycle_wait_ms.unwrap() >= 20.0);
        assert!(!serde_json::to_string(m)
            .unwrap()
            .contains("private-worker-secret"));
    }
    let mut headers = HeaderMap::new();
    headers.insert("x-studio-token", s.token.parse().unwrap());
    let response = chat(State(s.clone()), headers.clone(), Json(body.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    for _ in 0..100 {
        if s.requests.lock().await.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let before = serde_json::to_value(&*s.document.read().await).unwrap();
    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .header("x-studio-token", &s.token)
        .body(Body::from(
            json!({"messages":[],"max_tokens":129}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        proxy(State(s.clone()), request).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for _ in 0..100 {
        if s.requests.lock().await.len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let rows = s.requests.lock().await.clone();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].source, "chat");
    assert!(rows[1].gateway.as_ref().unwrap().admission_ms.unwrap() >= 20.0);
    let refused = rows[2].gateway.as_ref().unwrap();
    assert_eq!(refused.admission_rejected, Some(true));
    assert!(refused.dispatch_ms.is_none());
    assert_eq!(
        before,
        serde_json::to_value(&*s.document.read().await).unwrap()
    );
    s.stop().await.unwrap();
    task.abort();
}

#[cfg(unix)]
async fn prewarm_fixture(with_python: bool) -> (Arc<Studio>, PathBuf, tokio::task::JoinHandle<()>) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("soliton-autoprewarm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    if with_python {
        let path = dir.join("python");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nexec /bin/cat > '{}'\n",
                dir.join("input").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new().route(
        "/health",
        get(|| async { Json(json!({"control_ready":true,"inference_ready":true})) }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let s = studio();
    {
        let mut settings = s.settings.write().await;
        settings.engine = Engine::VllmApple;
        settings.vllm.executable = dir.join("mlx_lm.server").to_string_lossy().into();
        settings.vllm.evidence = "fixture".into();
    }
    let child = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    {
        let mut p = s.process.lock().await;
        p.group = child.id();
        p.child = Some(child);
        p.port = port;
        p.key = "private".into();
        p.startup = Some(Startup::new());
    }
    (s, dir, task)
}
#[cfg(unix)]
#[tokio::test]
async fn state_read_does_not_prepare_but_serve_monitor_does_without_prompts() {
    let (s, dir, backend) = prewarm_fixture(true).await;
    let snapshot = s.snapshot().await;
    assert!(snapshot["startup"]["counterPreparation"].is_null());
    assert!(!dir.join("input").exists());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let host = s.clone();
    let serving = tokio::spawn(async move {
        serve(listener, host, async {
            let _ = rx.await;
        })
        .await
        .unwrap()
    });
    for _ in 0..100 {
        if dir.join("input").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(dir.join("input").exists());
    assert_eq!(std::fs::metadata(dir.join("input")).unwrap().len(), 0);
    assert_eq!(
        s.process
            .lock()
            .await
            .startup
            .as_ref()
            .unwrap()
            .counter_preparation,
        Some("spawned")
    );
    assert!(s.requests.lock().await.is_empty());
    s.stop().await.unwrap();
    let _ = tx.send(());
    serving.await.unwrap();
    backend.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn stale_health_or_failed_spawn_cannot_prepare_again() {
    let (s, dir, backend) = prewarm_fixture(false).await;
    let settings = s.settings.read().await.clone();
    let port = s.process.lock().await.port;
    s.observe_startup_health(
        &settings,
        port,
        "stale-key",
        Some(&json!({"control_ready":true,"inference_ready":true})),
        true,
    )
    .await;
    assert!(s
        .process
        .lock()
        .await
        .startup
        .as_ref()
        .unwrap()
        .counter_preparation
        .is_none());
    s.observe_startup_health(
        &settings,
        port,
        "private",
        Some(&json!({"control_ready":false})),
        true,
    )
    .await;
    assert!(s
        .process
        .lock()
        .await
        .startup
        .as_ref()
        .unwrap()
        .counter_preparation
        .is_none());
    s.poll_startup().await;
    assert_eq!(
        s.process
            .lock()
            .await
            .startup
            .as_ref()
            .unwrap()
            .counter_preparation,
        Some("failed")
    );
    s.poll_startup().await;
    assert_eq!(
        s.process
            .lock()
            .await
            .startup
            .as_ref()
            .unwrap()
            .counter_preparation,
        Some("failed")
    );
    s.stop().await.unwrap();
    s.observe_startup_health(
        &settings,
        port,
        "private",
        Some(&json!({"control_ready":true})),
        true,
    )
    .await;
    assert!(!dir.join("input").exists());
    backend.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn launch_spans_keep_readiness_origin_and_unknown_values() {
    let mut startup = Startup::new();
    assert!(startup.snapshot()["launchPreparationMs"].is_null());
    assert!(startup.snapshot()["childSpawnMs"].is_null());
    let origin = startup.started;
    let preparation = origin - Duration::from_millis(120);
    let spawn = origin - Duration::from_millis(20);
    startup.record_launch(preparation, spawn, origin);
    startup.control_ms = Some(30);
    startup.inference_ms = Some(90);
    let snapshot = startup.snapshot();
    assert_eq!(snapshot["launchPreparationMs"], 100.0);
    assert_eq!(snapshot["childSpawnMs"], 20.0);
    assert_eq!(snapshot["modelLoadingObservedMs"], 60);
    assert_eq!(startup.started, origin);
}

#[cfg(unix)]
#[tokio::test]
async fn launch_spans_follow_real_launch_restart_failure_and_stop() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("soliton-launch-spans-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("serve")).unwrap();
    std::fs::write(dir.join("serve/server.py"), "# fixture").unwrap();
    let python = dir.join("python");
    // A delayed dependency check followed by an owned idle engine. No model needed.
    std::fs::write(
        &python,
        "#!/bin/sh\nif [ \"$1\" = '-c' ]; then /bin/sleep 0.05; exit 0; fi\nexec /bin/sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = Arc::new(Studio::new(dir.clone(), 0));
    {
        let mut settings = s.settings.write().await;
        settings.mock = true;
        settings.python = python.to_string_lossy().into();
    }
    s.start().await.unwrap();
    let first_session = s.process.lock().await.metrics_session.clone();
    let launch = s.process.lock().await.startup.as_ref().unwrap().snapshot();
    assert!(launch["launchPreparationMs"].as_f64().unwrap() >= 40.0);
    assert!(launch["childSpawnMs"].as_f64().unwrap() >= 0.0);
    assert!(launch["controlReadyObservedMs"].is_null());
    assert!(launch["modelLoadingObservedMs"].is_null());
    // Refusing a duplicate start must not replace the running session's timings.
    assert!(s.start().await.is_err());
    assert_eq!(
        s.process.lock().await.startup.as_ref().unwrap().snapshot(),
        launch
    );
    s.stop().await.unwrap();
    assert!(s.snapshot().await["startup"].is_null());
    s.start().await.unwrap();
    assert_ne!(s.process.lock().await.metrics_session, first_session);
    assert!(s
        .process
        .lock()
        .await
        .startup
        .as_ref()
        .unwrap()
        .control_ms
        .is_none());
    s.stop().await.unwrap();
    // A failed validation must not publish spans from the previous launch.
    s.settings.write().await.python = dir.join("missing").to_string_lossy().into();
    assert!(s.start().await.is_err());
    assert!(s.process.lock().await.startup.is_none());
    assert!(s.snapshot().await["startup"].is_null());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn prefix_metrics_reject_oversized_worker_response() {
    let s = studio();
    s.settings.write().await.engine = Engine::MlxPrefix;
    let padding = Arc::new(AtomicUsize::new(0));
    let response_padding = padding.clone();
    let router = Router::new().route(
        "/studio/ssd-cache",
        get(move || async move {
            Json(json!({
                "identity": "fixture",
                "padding": "x".repeat(response_padding.load(Ordering::SeqCst)),
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    {
        let mut process = s.process.lock().await;
        process.port = port;
        process.memory = Some(json!({"prefixIdentity": "fixture"}));
        process.child = Some(
            Command::new("/bin/sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        );
    }
    let mut headers = HeaderMap::new();
    headers.insert("x-studio-token", s.token.parse().unwrap());
    assert_eq!(
        prefix_stats(State(s.clone()), headers.clone())
            .await
            .status(),
        StatusCode::OK
    );
    padding.store(64 * 1024, Ordering::SeqCst);
    assert_eq!(
        prefix_stats(State(s.clone()), headers).await.status(),
        StatusCode::BAD_GATEWAY
    );
    s.process
        .lock()
        .await
        .child
        .take()
        .unwrap()
        .kill()
        .await
        .unwrap();
    server.abort();
}
