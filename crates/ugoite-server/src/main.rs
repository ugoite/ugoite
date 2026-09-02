use ugoite_server::{app, AppState};

const DEFAULT_SERVER_ADDRESS: &str = "127.0.0.1:8000";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = AppState::from_env()?;
    state.initialize_node().await?;
    let address = configured_server_address();
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("ugoite-server listening on http://{address}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn configured_server_address() -> String {
    server_address_from(std::env::var("UGOITE_SERVER_ADDRESS").ok())
}

fn server_address_from(override_address: Option<String>) -> String {
    override_address.unwrap_or_else(|| DEFAULT_SERVER_ADDRESS.to_string())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod server_address_tests {
    use super::*;

    #[test]
    fn req_sec_001_defaults_to_loopback() {
        assert_eq!(DEFAULT_SERVER_ADDRESS, "127.0.0.1:8000");
        assert_eq!(server_address_from(None), DEFAULT_SERVER_ADDRESS);
        assert_eq!(
            server_address_from(Some("0.0.0.0:8000".to_string())),
            "0.0.0.0:8000"
        );
    }
}
