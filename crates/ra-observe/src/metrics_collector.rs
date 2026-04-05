use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use ra_core::agent::AgentState;
use ra_core::event::{AgentEvent, StreamEvent};
use ra_core::metrics::{AgentMetrics, WorkflowMetrics};

/// A single log entry for the dashboard event log
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub agent_id: Uuid,
    pub event_type: LogEventType,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEventType {
    StateChange,
    Assistant,
    Result,
    Error,
    RateLimit,
    System,
}

impl LogEventType {
    pub fn label(&self) -> &'static str {
        match self {
            LogEventType::StateChange => "STATE",
            LogEventType::Assistant => "AGENT",
            LogEventType::Result => "DONE",
            LogEventType::Error => "ERROR",
            LogEventType::RateLimit => "RATE",
            LogEventType::System => "SYS",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DashboardData {
    pub agents: Vec<(Uuid, AgentMetrics)>,
    pub workflow: WorkflowMetrics,
    pub recent_events: Vec<LogEntry>,
}

const MAX_LOG_ENTRIES: usize = 500;

pub struct MetricsCollector {
    agent_metrics: DashMap<Uuid, AgentMetrics>,
    workflow_metrics: Arc<RwLock<WorkflowMetrics>>,
    event_log: Arc<RwLock<VecDeque<LogEntry>>>,
}

impl MetricsCollector {
    pub fn start(mut event_rx: broadcast::Receiver<AgentEvent>) -> Arc<Self> {
        let collector = Arc::new(Self {
            agent_metrics: DashMap::new(),
            workflow_metrics: Arc::new(RwLock::new(WorkflowMetrics::default())),
            event_log: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_LOG_ENTRIES))),
        });

        let collector_ref = collector.clone();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                collector_ref.handle_event(&event).await;
            }
        });

        collector
    }

    async fn handle_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::MetricsUpdated { agent_id, metrics } => {
                self.agent_metrics.insert(*agent_id, metrics.clone());
            }
            AgentEvent::StateChanged { agent_id, old, new } => {
                self.push_log(LogEntry {
                    timestamp: Utc::now(),
                    agent_id: *agent_id,
                    event_type: LogEventType::StateChange,
                    message: format!("{:?} -> {:?}", old, new),
                })
                .await;

                if new.is_terminal() {
                    let mut wm = self.workflow_metrics.write().await;
                    match new {
                        AgentState::Completed => wm.completed_agents += 1,
                        AgentState::Failed => wm.failed_agents += 1,
                        _ => {}
                    }
                    if let Some(m) = self.agent_metrics.get(agent_id) {
                        wm.total_cost_usd += m.total_cost_usd;
                        wm.total_input_tokens += m.input_tokens;
                        wm.total_output_tokens += m.output_tokens;
                    }
                }
            }
            AgentEvent::StreamLine {
                agent_id,
                event: stream_event,
            } => match stream_event {
                StreamEvent::Assistant { message, .. } => {
                    let snippet = extract_text_snippet(message, 120);
                    if !snippet.is_empty() {
                        self.push_log(LogEntry {
                            timestamp: Utc::now(),
                            agent_id: *agent_id,
                            event_type: LogEventType::Assistant,
                            message: snippet,
                        })
                        .await;
                    }
                }
                StreamEvent::Result {
                    result, is_error, ..
                } => {
                    let snippet: String = result.chars().take(120).collect();
                    self.push_log(LogEntry {
                        timestamp: Utc::now(),
                        agent_id: *agent_id,
                        event_type: if *is_error {
                            LogEventType::Error
                        } else {
                            LogEventType::Result
                        },
                        message: snippet,
                    })
                    .await;
                }
                StreamEvent::System { subtype, .. } => {
                    self.push_log(LogEntry {
                        timestamp: Utc::now(),
                        agent_id: *agent_id,
                        event_type: LogEventType::System,
                        message: subtype.clone(),
                    })
                    .await;
                }
            },
            AgentEvent::Error { agent_id, error } => {
                self.push_log(LogEntry {
                    timestamp: Utc::now(),
                    agent_id: *agent_id,
                    event_type: LogEventType::Error,
                    message: error.clone(),
                })
                .await;
            }
            AgentEvent::RateLimited {
                agent_id,
                retry_after_ms,
            } => {
                self.push_log(LogEntry {
                    timestamp: Utc::now(),
                    agent_id: *agent_id,
                    event_type: LogEventType::RateLimit,
                    message: format!("Rate limited, retry in {}ms", retry_after_ms),
                })
                .await;
            }
            AgentEvent::CheckpointSaved {
                agent_id,
                checkpoint_id,
            } => {
                self.push_log(LogEntry {
                    timestamp: Utc::now(),
                    agent_id: *agent_id,
                    event_type: LogEventType::System,
                    message: format!("Checkpoint saved: {}", &checkpoint_id.to_string()[..8]),
                })
                .await;
            }
            AgentEvent::ProcessExited {
                agent_id,
                exit_code,
            } => {
                self.push_log(LogEntry {
                    timestamp: Utc::now(),
                    agent_id: *agent_id,
                    event_type: if *exit_code == 0 {
                        LogEventType::System
                    } else {
                        LogEventType::Error
                    },
                    message: format!("Process exited (code {})", exit_code),
                })
                .await;
            }
        }
    }

    async fn push_log(&self, entry: LogEntry) {
        let mut log = self.event_log.write().await;
        log.push_back(entry);
        while log.len() > MAX_LOG_ENTRIES {
            log.pop_front();
        }
    }

    pub fn snapshot(&self) -> DashboardData {
        let agents: Vec<(Uuid, AgentMetrics)> = self
            .agent_metrics
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        let workflow = self.workflow_metrics.blocking_read().clone();
        let recent_events: Vec<LogEntry> = self.event_log.blocking_read().iter().cloned().collect();

        DashboardData {
            agents,
            workflow,
            recent_events,
        }
    }

    pub async fn snapshot_async(&self) -> DashboardData {
        let agents: Vec<(Uuid, AgentMetrics)> = self
            .agent_metrics
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        let workflow = self.workflow_metrics.read().await.clone();
        let recent_events: Vec<LogEntry> = self.event_log.read().await.iter().cloned().collect();

        DashboardData {
            agents,
            workflow,
            recent_events,
        }
    }
}

/// Extract a text snippet from a JSON message value
fn extract_text_snippet(value: &serde_json::Value, max_len: usize) -> String {
    // Try common patterns: "content" string, "content" array, "text" field
    if let Some(s) = value.as_str() {
        return s.chars().take(max_len).collect();
    }
    if let Some(content) = value.get("content") {
        if let Some(s) = content.as_str() {
            return s.chars().take(max_len).collect();
        }
        if let Some(arr) = content.as_array() {
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    return text.chars().take(max_len).collect();
                }
            }
        }
    }
    if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
        return text.chars().take(max_len).collect();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_snippet_string() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_text_snippet(&val, 5), "hello");
    }

    #[test]
    fn test_extract_text_snippet_content_obj() {
        let val = serde_json::json!({"content": "some text here"});
        assert_eq!(extract_text_snippet(&val, 9), "some text");
    }

    #[test]
    fn test_extract_text_snippet_content_array() {
        let val = serde_json::json!({"content": [{"type": "text", "text": "nested text"}]});
        assert_eq!(extract_text_snippet(&val, 100), "nested text");
    }

    #[test]
    fn test_extract_text_snippet_empty() {
        let val = serde_json::json!({"foo": 42});
        assert_eq!(extract_text_snippet(&val, 100), "");
    }

    #[test]
    fn test_log_event_type_labels() {
        assert_eq!(LogEventType::StateChange.label(), "STATE");
        assert_eq!(LogEventType::Error.label(), "ERROR");
        assert_eq!(LogEventType::RateLimit.label(), "RATE");
    }
}
