#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod hub;
mod installation;
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
struct Runtime {
    installation: std::sync::Mutex<Option<serde_json::Value>>,
    install_data: PathBuf,
    resources: PathBuf,
    install_lock: tokio::sync::Mutex<()>,
    download: tokio::sync::Mutex<()>,
    cancel_download: std::sync::atomic::AtomicBool,
    studio: Arc<studio_core::Studio>,
    panels: tokio::sync::Mutex<()>,
    shutdown: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
fn provision(resources: &std::path::Path, data: &std::path::Path) -> Result<(), serde_json::Value> {
    installation::preflight(data)?;
    install_bundled_engine(&resources.join("strata.tar.gz"), data)
        .map_err(|e| installation::failure(data, "installation", &e))?;
    install_bundled_python(&resources.join("python.tar.gz"), data)
        .map_err(|e| installation::failure(data, "installation", &e))?;
    Ok(())
}
#[tauri::command]
fn installation_status(runtime: State<Runtime>) -> Option<serde_json::Value> {
    runtime.installation.lock().unwrap().clone()
}
#[tauri::command]
async fn retry_installation(runtime: State<'_, Runtime>) -> Result<(), String> {
    let _guard = runtime.install_lock.lock().await;
    if runtime.installation.lock().unwrap().is_none() {
        return Ok(());
    }
    let resources = runtime.resources.clone();
    let data = runtime.install_data.clone();
    let result = tauri::async_runtime::spawn_blocking(move || provision(&resources, &data))
        .await
        .map_err(|e| e.to_string())?;
    match result {
        Ok(()) => {
            *runtime.installation.lock().unwrap() = None;
            Ok(())
        }
        Err(error) => {
            *runtime.installation.lock().unwrap() = Some(error);
            Err("Installation failed".into())
        }
    }
}
#[tauri::command]
fn connection(runtime: State<Runtime>) -> serde_json::Value {
    serde_json::json!({"url":format!("http://127.0.0.1:{}",runtime.studio.port),"token":runtime.studio.token})
}
#[tauri::command]
async fn trace_workspace_drag(
    window: tauri::WebviewWindow,
    runtime: State<'_, Runtime>,
    event: serde_json::Value,
) -> Result<(), String> {
    runtime
        .studio
        .docking_diagnostics
        .lock()
        .await
        .record(window.label(), event);
    Ok(())
}
#[tauri::command]
async fn float_panel(
    app: tauri::AppHandle,
    runtime: State<'_, Runtime>,
    panel: String,
) -> Result<(), String> {
    let _guard = runtime.panels.lock().await;
    let mut document = runtime.studio.document.write().await;
    if !["engine", "api", "logs"].contains(&panel.as_str()) && !document.layout.is_group(&panel) {
        return Err("Unknown or stale panel group".into());
    }
    let workspace = document.layout.is_group(&panel);
    let host = document.host_for(&panel).to_string();
    if workspace && host != "main" {
        if let Some(w) = app.get_webview_window(&format!("panel-{host}")) {
            w.show().map_err(|e| e.to_string())?;
            w.set_focus().map_err(|e| e.to_string())?;
            return Ok(());
        }
        document.release_host(&host);
    }
    let label = format!("panel-{panel}");
    if let Some(w) = app.get_webview_window(&label) {
        if workspace {
            document.float_group(&panel, &panel)?;
        }
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(format!("index.html?panel={panel}").into()),
    )
    .title(format!("Soliton · {panel}"))
    .inner_size(
        if workspace || panel == "logs" {
            900.0
        } else {
            380.0
        },
        640.0,
    )
    .min_inner_size(320.0, 320.0)
    .disable_drag_drop_handler()
    .build()
    .map_err(|e| e.to_string())?;
    if workspace {
        document.float_group(&panel, &panel)?;
    } else {
        document.detached.retain(|id| id != &panel);
        document.detached.push(panel.clone());
    }
    drop(document);
    let studio = runtime.studio.clone();
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let studio = studio.clone();
            let panel = panel.clone();
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let runtime = handle.state::<Runtime>();
                let _guard = runtime.panels.lock().await;
                if handle
                    .get_webview_window(&format!("panel-{panel}"))
                    .is_some()
                {
                    return;
                }
                let mut document = studio.document.write().await;
                if studio_core::docking::workspace_host(&panel) {
                    document.release_host(&panel);
                } else {
                    document.detached.retain(|p| p != &panel);
                }
            });
        }
    });
    Ok(())
}
#[tauri::command]
async fn dock_panel(
    app: tauri::AppHandle,
    runtime: State<'_, Runtime>,
    panel: String,
) -> Result<(), String> {
    let _guard = runtime.panels.lock().await;
    if ![
        "engine",
        "api",
        "logs",
        "workspace",
        "group-chat",
        "group-logs",
    ]
    .contains(&panel.as_str())
    {
        return Err("Unknown panel".into());
    }
    if studio_core::docking::workspace_host(&panel) {
        let retired = runtime.studio.document.write().await.return_group(&panel);
        if let Some(host) = retired {
            retire_workspace_hosts(&app, runtime.studio.clone(), vec![host]).await?;
        }
    } else {
        if let Some(w) = app.get_webview_window(&format!("panel-{panel}")) {
            w.close().map_err(|e| e.to_string())?;
        }
        runtime
            .studio
            .document
            .write()
            .await
            .detached
            .retain(|p| p != &panel);
    }
    Ok(())
}
/// Keep a generating source WebView alive but hidden until its stream finishes.
async fn retire_workspace_hosts(
    app: &tauri::AppHandle,
    studio: Arc<studio_core::Studio>,
    hosts: Vec<String>,
) -> Result<(), String> {
    for host in hosts {
        let document = studio.document.read().await;
        if document.workspace_hosts.values().any(|id| id == &host) {
            continue;
        }
        let generating = document.generating;
        drop(document);
        if let Some(window) = app.get_webview_window(&format!("panel-{host}")) {
            if !generating {
                window.close().map_err(|e| e.to_string())?;
                continue;
            }
            window.hide().map_err(|e| e.to_string())?;
            let handle = app.clone();
            let studio = studio.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    let runtime = handle.state::<Runtime>();
                    let _guard = runtime.panels.lock().await;
                    let document = studio.document.read().await;
                    if document.workspace_hosts.values().any(|id| id == &host) {
                        return;
                    }
                    if document.generating {
                        continue;
                    }
                    drop(document);
                    if let Some(w) = handle.get_webview_window(&format!("panel-{host}")) {
                        let _ = w.close();
                    }
                    return;
                }
            });
        }
    }
    Ok(())
}
#[tauri::command]
async fn dock_workspace_tab(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    runtime: State<'_, Runtime>,
    tab: String,
    target: String,
    placement: String,
) -> Result<(), String> {
    let _guard = runtime.panels.lock().await;
    let destination = if window.label() == "main" {
        "main"
    } else {
        window
            .label()
            .strip_prefix("panel-")
            .ok_or("Invalid workspace window")?
    };
    let retired = runtime.studio.document.write().await.dock_workspace(
        &tab,
        &target,
        &placement,
        destination,
    )?;
    // Reconcile windows even if writing settings fails: the in-memory document is authoritative.
    let saved = runtime.studio.persist().await;
    retire_workspace_hosts(&app, runtime.studio.clone(), retired).await?;
    window.set_focus().map_err(|e| e.to_string())?;
    saved
}
#[tauri::command]
async fn focus_workspace_tab(
    app: tauri::AppHandle,
    runtime: State<'_, Runtime>,
    tab: String,
) -> Result<(), String> {
    let document = runtime.studio.document.read().await;
    let group = document
        .layout
        .groups()
        .into_iter()
        .find(|g| g.tabs.contains(&tab))
        .ok_or("Unknown workspace tab")?;
    let host = document.host_for(&group.id);
    let label = if host != "main" {
        format!("panel-{host}")
    } else {
        "main".into()
    };
    drop(document);
    app.get_webview_window(&label)
        .ok_or("Workspace window not found")?
        .set_focus()
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn select_path(app: tauri::AppHandle, kind: String) -> Result<Option<String>, String> {
    if !["root", "config", "python", "models", "evidence"].contains(&kind.as_str()) {
        return Err("Unknown selection type".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let selected = if kind == "root" || kind == "models" {
            dialog.blocking_pick_folder()
        } else if kind == "evidence" {
            dialog
                .add_filter("Qualification evidence", &["json"])
                .blocking_pick_file()
        } else if kind == "config" {
            dialog
                .add_filter("Strata config", &["json"])
                .blocking_pick_file()
        } else {
            dialog.blocking_pick_file()
        };
        selected
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn quit_app(app: tauri::AppHandle, runtime: State<'_, Runtime>) -> Result<(), String> {
    runtime.studio.shutdown().await.map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}
#[tauri::command]
async fn export_conversation(
    app: tauri::AppHandle,
    runtime: State<'_, Runtime>,
) -> Result<Option<String>, String> {
    let messages = runtime.studio.document.read().await.messages.clone();
    let data = serde_json::to_vec_pretty(&serde_json::json!({"version": 1, "messages": messages}))
        .map_err(|e| e.to_string())?;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name("Soliton-conversation.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(file) = selected {
        let path = file.into_path().map_err(|e| e.to_string())?;
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    } else {
        Ok(None)
    }
}
#[tauri::command]
async fn check_software_update() -> Result<serde_json::Value, String> {
    let response = reqwest::Client::builder()
        .user_agent("Soliton-update-check")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .get("https://api.github.com/repos/matsushibadenki/Soliton/releases/latest")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(serde_json::json!({"status":"noRelease", "current":env!("CARGO_PKG_VERSION")}));
    }
    let release: serde_json::Value = response
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or("Release has no version tag")?;
    let latest = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|_| "Release tag is not a semantic version")?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"status":if latest > current {"available"} else {"current"}, "current":current.to_string(), "latest":latest.to_string()}),
    )
}

#[tauri::command]
fn open_releases_page() -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("https://github.com/matsushibadenki/Soliton/releases/latest")
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Unable to open releases page".into())
    }
}

#[tauri::command]
fn cancel_model_download(runtime: State<'_, Runtime>) {
    runtime
        .cancel_download
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
async fn download_huggingface_file(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    runtime: State<'_, Runtime>,
    url: String,
    token: Option<String>,
) -> Result<String, String> {
    let _permit = runtime
        .download
        .try_lock()
        .map_err(|_| "A model download is already running")?;
    runtime
        .cancel_download
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    hub::download(
        &base,
        &url,
        &token.unwrap_or_default(),
        &runtime.cancel_download,
        |received, total| {
            let _ = window.emit(
                "model-download-progress",
                serde_json::json!({"received":received,"total":total}),
            );
        },
    )
    .await
}
// Preserve interrupted or damaged installations for recovery; never delete user data.
fn preserve_incomplete_install(root: &std::path::Path) -> Result<(), String> {
    if root.exists() {
        let backup = root.with_file_name(format!(
            "{}.recovery-{}",
            root.file_name().unwrap().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        std::fs::rename(root, backup)
            .map_err(|e| format!("Cannot preserve incomplete installation: {e}"))?;
    }
    Ok(())
}

fn install_bundled_python(
    archive: &std::path::Path,
    app_data: &std::path::Path,
) -> Result<PathBuf, String> {
    let root = app_data.join("python-3.12.15");
    if root.join("bin/python3").is_file() {
        let healthy = std::process::Command::new(root.join("bin/python3"))
            .args([
                "-I",
                "-c",
                "import ssl, numpy, jinja2, regex, PIL, requests, yaml, psutil",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if healthy {
            return Ok(root);
        }
    }
    preserve_incomplete_install(&root)?;
    std::fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let staging = app_data.join(format!(".python-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let status = std::process::Command::new("/usr/bin/tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Python extraction failed".into());
        }
        let status = std::process::Command::new(staging.join("bin/python3"))
            .args([
                "-I",
                "-c",
                "import ssl, numpy, jinja2, regex, PIL, requests, yaml, psutil",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Bundled Python dependency validation failed".into());
        }
        std::fs::rename(&staging, &root).map_err(|e| e.to_string())?;
        Ok(root)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(staging);
    }
    result
}
// Install the immutable bundled source into a writable per-user directory.  A
// partially extracted tree is never treated as an installation.
fn install_bundled_engine(
    archive: &std::path::Path,
    app_data: &std::path::Path,
) -> Result<PathBuf, String> {
    let root = app_data.join("Strata-metal-v1");
    if root.join("serve/server.py").is_file()
        && root.join("metal/setup_mac.py").is_file()
        && (!root.join(".bundled-metal.json").exists() || root.join("build-metal/strata").is_file())
    {
        return Ok(root);
    }
    preserve_incomplete_install(&root)?;
    std::fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let staging = app_data.join(format!(".strata-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let status = std::process::Command::new("/usr/bin/tar")
            .args(["-xzf"])
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success()
            || !staging.join("serve/server.py").is_file()
            || !staging.join("metal/setup_mac.py").is_file()
        {
            return Err("Bundled Strata extraction failed".to_string());
        }
        std::fs::rename(&staging, &root).map_err(|e| e.to_string())?;
        Ok(root)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = if let Some(path) = std::env::var_os("SOLITON_DATA_DIR") {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err("SOLITON_DATA_DIR must be absolute".into());
                }
                path
            } else {
                app.path().app_data_dir()?
            };
            let resources = app.path().resource_dir()?;
            let managed = !cfg!(debug_assertions) && std::env::var_os("STRATA_ROOT").is_none();
            let install_error = if managed {
                provision(&resources, &app_data).err()
            } else {
                None
            };
            let root = if let Some(root) = std::env::var_os("STRATA_ROOT") {
                PathBuf::from(root)
            } else if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Strata")
            } else {
                app_data.join("Strata-metal-v1")
            };
            let legacy = app_data
                .parent()
                .unwrap_or(&app_data)
                .join("dev.strata.studio/studio.json");
            let mut studio = studio_core::Studio::new(root, 8080);
            if managed {
                studio.settings.get_mut().python = app_data
                    .join("python-3.12.15/bin/python3")
                    .to_string_lossy()
                    .into();
            }
            // Match the headless runtime's explicit state override for isolated native QA.
            let studio = Arc::new(if let Some(path) = std::env::var_os("STUDIO_STATE_PATH") {
                studio.with_storage(PathBuf::from(path))
            } else {
                studio.with_legacy_storage(app_data.join("studio.json"), legacy)
            });
            let listener = std::net::TcpListener::bind("127.0.0.1:8080")?;
            listener.set_nonblocking(true)?;
            let (tx, rx) = tokio::sync::oneshot::channel();
            let s = studio.clone();
            tauri::async_runtime::spawn(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("nonblocking listener");
                let _ = studio_core::serve(listener, s, async {
                    let _ = rx.await;
                })
                .await;
            });
            // Terminal/test termination must run the same ordered cleanup as Quit.
            // OS signal termination does not necessarily emit Tauri's Exit event.
            #[cfg(unix)]
            {
                let signal_studio = studio.clone();
                let signal_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let (Ok(mut interrupt), Ok(mut terminate)) = (
                        signal(SignalKind::interrupt()),
                        signal(SignalKind::terminate()),
                    ) else {
                        return;
                    };
                    loop {
                        tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
                        if signal_studio.shutdown().await.is_ok() {
                            signal_app.exit(0);
                            break;
                        }
                        eprintln!("Soliton shutdown incomplete; retry stop before exiting");
                    }
                });
            }
            app.manage(Runtime {
                installation: std::sync::Mutex::new(install_error),
                install_data: app_data,
                resources,
                install_lock: tokio::sync::Mutex::new(()),
                download: tokio::sync::Mutex::new(()),
                cancel_download: std::sync::atomic::AtomicBool::new(false),
                studio,
                panels: tokio::sync::Mutex::new(()),
                shutdown: std::sync::Mutex::new(Some(tx)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection,
            installation_status,
            retry_installation,
            download_huggingface_file,
            cancel_model_download,
            check_software_update,
            open_releases_page,
            quit_app,
            export_conversation,
            select_path,
            float_panel,
            dock_panel,
            focus_workspace_tab,
            dock_workspace_tab,
            trace_workspace_drag
        ])
        .build(tauri::generate_context!())
        .expect("Cannot start Soliton")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let runtime = app.state::<Runtime>();
                tauri::async_runtime::block_on(runtime.studio.shutdown()).ok();
                if let Some(tx) = runtime.shutdown.lock().unwrap().take() {
                    let _ = tx.send(());
                };
            }
        });
}

#[cfg(test)]
mod bundled_engine_tests {
    use super::install_bundled_engine;
    #[test]
    fn install_is_atomic_and_preserves_existing_tree() {
        let base =
            std::env::temp_dir().join(format!("soliton-install-test-{}", uuid::Uuid::new_v4()));
        let source = base.join("source");
        let data = base.join("data");
        std::fs::create_dir_all(source.join("serve")).unwrap();
        std::fs::create_dir_all(source.join("metal")).unwrap();
        std::fs::write(source.join("serve/server.py"), "original").unwrap();
        std::fs::write(source.join("metal/setup_mac.py"), "setup").unwrap();
        let archive = base.join("strata.tar.gz");
        assert!(std::process::Command::new("/usr/bin/tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg(".")
            .status()
            .unwrap()
            .success());
        let root = install_bundled_engine(&archive, &data).unwrap();
        std::fs::write(root.join("serve/server.py"), "user data").unwrap();
        install_bundled_engine(&archive, &data).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("serve/server.py")).unwrap(),
            "user data"
        );
        std::fs::remove_file(root.join("serve/server.py")).unwrap();
        std::fs::write(root.join("keep-user-config.json"), "preserve").unwrap();
        install_bundled_engine(&archive, &data).unwrap();
        assert!(root.join("serve/server.py").is_file());
        let backup = std::fs::read_dir(&data)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().contains(".recovery-"))
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read_to_string(backup.join("keep-user-config.json")).unwrap(),
            "preserve"
        );
        std::fs::remove_dir_all(base).unwrap();
    }
}
