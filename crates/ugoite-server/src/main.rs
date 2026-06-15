use ugoite_server::{app, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState::from_env()?;
    let address =
        std::env::var("UGOITE_SERVER_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8000".to_string());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("ugoite-server listening on http://{address}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
