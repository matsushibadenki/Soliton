//! Authenticated loopback-only fixture for cross-WebView IPC tests; not part of studio-server.
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use studio_core::Studio;
async fn command(
    State(studio): State<Arc<Studio>>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if headers.get("x-studio-token").and_then(|v| v.to_str().ok()) != Some(studio.token.as_str()) {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".into()));
    }
    let mut d = studio.document.write().await;
    let text = |key| value[key].as_str().unwrap_or("");
    let retired = match text("command") {
        "float" => {
            d.float_group(text("group"), text("group"))
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            vec![]
        }
        "dock" => d
            .dock_workspace(text("tab"), text("target"), text("placement"), text("host"))
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?,
        "return" => d.return_group(text("group")).into_iter().collect(),
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown fixture command".into())),
    };
    drop(d);
    studio
        .persist()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({"retired":retired})))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::var("STUDIO_TEST_STATE_PATH")?);
    if !path.starts_with(std::env::temp_dir()) && !path.starts_with("/tmp") {
        return Err("Fixture state must live in a temporary directory".into());
    }
    let studio =
        Arc::new(Studio::new(std::env::current_dir()?.join("Strata"), 8081).with_storage(path));
    let router = studio_core::router(studio.clone()).merge(
        Router::new()
            .route("/fixture/command", post(command))
            .with_state(studio),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8081").await?;
    println!("Docking fixture: http://127.0.0.1:8081");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
