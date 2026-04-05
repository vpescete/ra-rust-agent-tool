//! IPC client: connects to the Unix socket and receives events for the dashboard.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;

use ra_core::event::AgentEvent;
use ra_core::ipc::{default_socket_path, IpcEvent};

use crate::metrics_collector::MetricsCollector;

/// Connect to the IPC socket and feed events to the MetricsCollector.
/// Returns the collector for use by the dashboard.
pub async fn connect_and_collect(
    socket_path: Option<&str>,
) -> anyhow::Result<Arc<MetricsCollector>> {
    let path = socket_path
        .map(|s| s.to_string())
        .unwrap_or_else(default_socket_path);

    let stream = UnixStream::connect(&path).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to IPC socket at {}.\nIs the RA MCP server running? (Check: claude mcp list)\nError: {}",
            path,
            e
        )
    })?;

    // Create a broadcast channel to feed the MetricsCollector
    let (event_tx, event_rx) = tokio::sync::broadcast::channel::<AgentEvent>(512);
    let collector = MetricsCollector::start(event_rx);

    // Spawn reader task: read JSONL from socket, convert to AgentEvent, broadcast
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(ipc_event) = IpcEvent::from_json_line(&line) {
                if let Some(agent_event) = ipc_to_agent_event(ipc_event) {
                    let _ = event_tx.send(agent_event);
                }
            }
        }
    });

    Ok(collector)
}

/// Convert IpcEvent back to AgentEvent for the MetricsCollector
fn ipc_to_agent_event(ipc: IpcEvent) -> Option<AgentEvent> {
    match ipc {
        IpcEvent::StateChanged { agent_id, old, new } => {
            Some(AgentEvent::StateChanged { agent_id, old, new })
        }
        IpcEvent::MetricsUpdated { agent_id, metrics } => {
            Some(AgentEvent::MetricsUpdated { agent_id, metrics })
        }
        IpcEvent::StreamLine {
            agent_id,
            event_type,
            message,
        } => {
            // Convert back to a StreamEvent for the collector
            let stream_event = match event_type.as_str() {
                "assistant" => ra_core::event::StreamEvent::Assistant {
                    message: serde_json::Value::String(message),
                    usage: None,
                },
                "result" => ra_core::event::StreamEvent::Result {
                    subtype: "success".to_string(),
                    is_error: false,
                    duration_ms: 0,
                    num_turns: 0,
                    result: message,
                    session_id: None,
                    total_cost_usd: 0.0,
                    usage: None,
                    model_usage: serde_json::Value::Null,
                },
                "error" => ra_core::event::StreamEvent::Result {
                    subtype: "error".to_string(),
                    is_error: true,
                    duration_ms: 0,
                    num_turns: 0,
                    result: message,
                    session_id: None,
                    total_cost_usd: 0.0,
                    usage: None,
                    model_usage: serde_json::Value::Null,
                },
                _ => ra_core::event::StreamEvent::System {
                    subtype: message,
                    data: serde_json::Value::Null,
                },
            };
            Some(AgentEvent::StreamLine {
                agent_id,
                event: stream_event,
            })
        }
        IpcEvent::ProcessExited {
            agent_id,
            exit_code,
        } => Some(AgentEvent::ProcessExited {
            agent_id,
            exit_code,
        }),
        IpcEvent::Error { agent_id, error } => Some(AgentEvent::Error { agent_id, error }),
        IpcEvent::RateLimited {
            agent_id,
            retry_after_ms,
        } => Some(AgentEvent::RateLimited {
            agent_id,
            retry_after_ms,
        }),
    }
}
