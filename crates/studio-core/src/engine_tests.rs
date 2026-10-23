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
