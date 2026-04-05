mod handler;
mod ipc_broadcaster;
mod protocol;
mod server;
mod tools;

use handler::Handler;
use ipc_broadcaster::IpcBroadcaster;
use ra_core::event::AgentEvent;
use ra_core::ipc::default_socket_path;
use ra_core::RuntimeConfig;
use server::McpServer;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to stderr (stdout is reserved for JSON-RPC)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // Load config
    let config = load_config();

    // Create global event channel
    let (event_tx, event_rx) = broadcast::channel::<AgentEvent>(1024);

    // Start IPC broadcaster (Unix socket for dashboard)
    let socket_path = default_socket_path();
    let broadcaster = IpcBroadcaster::new(socket_path);
    broadcaster.start(event_rx);

    tracing::info!("RA MCP Server starting (IPC socket active)");

    let handler = Handler::new(config, event_tx);
    let server = McpServer::new(handler);

    server.run().await
}

fn load_config() -> RuntimeConfig {
    let config_path = RuntimeConfig::expand_path("~/.ra/config.toml");
    if std::path::Path::new(&config_path).exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = RuntimeConfig::from_toml(&content) {
                return config;
            }
        }
    }
    RuntimeConfig::default()
}
