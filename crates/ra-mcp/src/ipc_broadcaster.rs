//! IPC broadcaster: listens on a Unix socket and sends events to all connected clients.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, RwLock};

use ra_core::event::AgentEvent;
use ra_core::ipc::IpcEvent;

/// Manages the Unix socket server that broadcasts events to dashboard clients.
pub struct IpcBroadcaster {
    socket_path: String,
}

impl IpcBroadcaster {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    /// Start the IPC broadcaster. Spawns background tasks for:
    /// 1. Accepting new client connections on the Unix socket
    /// 2. Forwarding events from the broadcast channel to all connected clients
    pub fn start(&self, mut event_rx: broadcast::Receiver<AgentEvent>) {
        // Remove stale socket file
        let _ = std::fs::remove_file(&self.socket_path);

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(&self.socket_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let socket_path = self.socket_path.clone();
        let clients: Arc<RwLock<Vec<UnixStream>>> = Arc::new(RwLock::new(Vec::new()));

        // Task 1: Accept new connections
        let clients_accept = clients.clone();
        let path = socket_path.clone();
        tokio::spawn(async move {
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind IPC socket {}: {}", path, e);
                    return;
                }
            };
            tracing::info!("IPC socket listening on {}", path);

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        tracing::debug!("Dashboard client connected");
                        clients_accept.write().await.push(stream);
                    }
                    Err(e) => {
                        tracing::error!("IPC accept error: {}", e);
                        break;
                    }
                }
            }
        });

        // Task 2: Forward events to all connected clients
        let clients_send = clients;
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                let ipc_event: IpcEvent = (&event).into();
                let mut line = ipc_event.to_json_line();
                line.push('\n');
                let bytes = line.as_bytes();

                let mut clients = clients_send.write().await;
                let mut disconnected = Vec::new();

                for (i, client) in clients.iter_mut().enumerate() {
                    if let Err(_) = client.write_all(bytes).await {
                        disconnected.push(i);
                    }
                }

                // Remove disconnected clients (reverse order to maintain indices)
                for i in disconnected.into_iter().rev() {
                    tracing::debug!("Dashboard client disconnected");
                    clients.remove(i);
                }
            }
        });
    }

    /// Cleanup socket file on shutdown
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for IpcBroadcaster {
    fn drop(&mut self) {
        self.cleanup();
    }
}
