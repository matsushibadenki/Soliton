use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var_os("STRATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("Strata"));
    let port = std::env::var("STUDIO_PORT")
        .unwrap_or("8080".into())
        .parse()?;
    let storage = std::env::var_os("STUDIO_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join(".studio-state.json"));
    let state = Arc::new(studio_core::Studio::new(root, port).with_storage(storage));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("Soliton: http://127.0.0.1:{port}");
    let cleanup = state.clone();
    axum_serve(listener, state, cleanup).await
}
async fn axum_serve(
    listener: tokio::net::TcpListener,
    state: Arc<studio_core::Studio>,
    cleanup: Arc<studio_core::Studio>,
) -> Result<(), Box<dyn std::error::Error>> {
    let signal_cleanup = cleanup.clone();
    studio_core::serve(listener, state, async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = signal_cleanup.shutdown().await;
    })
    .await?;
    cleanup.shutdown().await.map_err(std::io::Error::other)?;
    Ok(())
}
