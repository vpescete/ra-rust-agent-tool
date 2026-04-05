//! IPC types shared between MCP server (sender) and dashboard (receiver).
//! Protocol: Unix socket, newline-delimited JSON messages.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentState;
use crate::metrics::AgentMetrics;

/// Default socket path
pub fn default_socket_path() -> String {
    crate::config::RuntimeConfig::expand_path("~/.ra/ra.sock")
}

/// Wire-format event sent over the Unix socket.
/// This is a simplified, serializable version of AgentEvent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcEvent {
    #[serde(rename = "state_changed")]
    StateChanged {
        agent_id: Uuid,
        old: AgentState,
        new: AgentState,
    },

    #[serde(rename = "metrics_updated")]
    MetricsUpdated {
        agent_id: Uuid,
        metrics: AgentMetrics,
    },

    #[serde(rename = "stream_line")]
    StreamLine {
        agent_id: Uuid,
        event_type: String, // "assistant", "result", "system"
        message: String,
    },

    #[serde(rename = "process_exited")]
    ProcessExited { agent_id: Uuid, exit_code: i32 },

    #[serde(rename = "error")]
    Error { agent_id: Uuid, error: String },

    #[serde(rename = "rate_limited")]
    RateLimited { agent_id: Uuid, retry_after_ms: u64 },
}

impl IpcEvent {
    /// Serialize to a single JSON line (no newline at end)
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserialize from a JSON line
    pub fn from_json_line(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim()).ok()
    }
}

/// Convert internal AgentEvent to wire IpcEvent
impl From<&crate::event::AgentEvent> for IpcEvent {
    fn from(event: &crate::event::AgentEvent) -> Self {
        use crate::event::AgentEvent;
        match event {
            AgentEvent::StateChanged { agent_id, old, new } => IpcEvent::StateChanged {
                agent_id: *agent_id,
                old: *old,
                new: *new,
            },
            AgentEvent::MetricsUpdated { agent_id, metrics } => IpcEvent::MetricsUpdated {
                agent_id: *agent_id,
                metrics: metrics.clone(),
            },
            AgentEvent::StreamLine {
                agent_id,
                event: stream_event,
            } => {
                let (event_type, message) = match stream_event {
                    crate::event::StreamEvent::Assistant { message, .. } => {
                        ("assistant".to_string(), format!("{}", message))
                    }
                    crate::event::StreamEvent::Result {
                        result, is_error, ..
                    } => {
                        let t = if *is_error { "error" } else { "result" };
                        (t.to_string(), result.clone())
                    }
                    crate::event::StreamEvent::System { subtype, .. } => {
                        ("system".to_string(), subtype.clone())
                    }
                };
                IpcEvent::StreamLine {
                    agent_id: *agent_id,
                    event_type,
                    message: message.chars().take(500).collect(),
                }
            }
            AgentEvent::ProcessExited {
                agent_id,
                exit_code,
            } => IpcEvent::ProcessExited {
                agent_id: *agent_id,
                exit_code: *exit_code,
            },
            AgentEvent::Error { agent_id, error } => IpcEvent::Error {
                agent_id: *agent_id,
                error: error.clone(),
            },
            AgentEvent::RateLimited {
                agent_id,
                retry_after_ms,
            } => IpcEvent::RateLimited {
                agent_id: *agent_id,
                retry_after_ms: *retry_after_ms,
            },
            AgentEvent::CheckpointSaved {
                agent_id,
                checkpoint_id,
            } => IpcEvent::StreamLine {
                agent_id: *agent_id,
                event_type: "system".to_string(),
                message: format!("checkpoint:{}", checkpoint_id),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_roundtrip() {
        let event = IpcEvent::StateChanged {
            agent_id: Uuid::new_v4(),
            old: AgentState::Pending,
            new: AgentState::Running,
        };
        let json = event.to_json_line();
        let parsed = IpcEvent::from_json_line(&json).unwrap();
        match parsed {
            IpcEvent::StateChanged { old, new, .. } => {
                assert_eq!(old, AgentState::Pending);
                assert_eq!(new, AgentState::Running);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_serialize_metrics() {
        let event = IpcEvent::MetricsUpdated {
            agent_id: Uuid::new_v4(),
            metrics: AgentMetrics {
                input_tokens: 1000,
                output_tokens: 500,
                total_cost_usd: 0.05,
                ..Default::default()
            },
        };
        let json = event.to_json_line();
        assert!(json.contains("metrics_updated"));
        assert!(json.contains("1000"));
    }

    #[test]
    fn test_from_agent_event() {
        let agent_event = crate::event::AgentEvent::Error {
            agent_id: Uuid::new_v4(),
            error: "test error".to_string(),
        };
        let ipc: IpcEvent = (&agent_event).into();
        match ipc {
            IpcEvent::Error { error, .. } => assert_eq!(error, "test error"),
            _ => panic!("Wrong variant"),
        }
    }
}
