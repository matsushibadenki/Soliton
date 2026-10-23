#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
struct Runtime {
    studio: Arc<studio_core::Studio>,
    panels: tokio::sync::Mutex<()>,
    shutdown: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
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
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = std::env::var_os("STRATA_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Strata"));
            let app_data = app.path().app_data_dir()?;
            let legacy = app_data
                .parent()
                .unwrap_or(&app_data)
                .join("dev.strata.studio/studio.json");
            let studio = studio_core::Studio::new(root, 8080);
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
                studio,
                panels: tokio::sync::Mutex::new(()),
                shutdown: std::sync::Mutex::new(Some(tx)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection,
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
