//! Loopback-only qualification host using the production relay.
use axum::{
    extract::ws::WebSocketUpgrade,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("OMNI_PROBE_KEY")?;
    let app = Router::new().route(
        "/v1/realtime",
        get(move |headers: HeaderMap, ws: WebSocketUpgrade| {
            let key = key.clone();
            async move {
                if headers.get("authorization").and_then(|v| v.to_str().ok())
                    != Some(&format!("Bearer {key}"))
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                ws.max_message_size(1024 * 1024)
                    .on_upgrade(move |socket| async move {
                        if let Err(err) =
                            studio_core::omni_realtime::relay(socket, 19341, key, 4096).await
                        {
                            eprintln!("{err}");
                        }
                    })
            }
        }),
    );
    axum::serve(tokio::net::TcpListener::bind("127.0.0.1:19342").await?, app).await?;
    Ok(())
}
