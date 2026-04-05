use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentState;
use crate::metrics::AgentMetrics;

/// Maps the JSONL output from `claude --output-format stream-json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "system")]
    System {
        #[serde(default)]
        subtype: String,
        #[serde(flatten)]
        data: serde_json::Value,
    },

    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: serde_json::Value,
        #[serde(default)]
        usage: Option<UsageInfo>,
    },

    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        subtype: String,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        duration_ms: u64,
        #[serde(default)]
        num_turns: u32,
        #[serde(default)]
        result: String,
        #[serde(default)]
        session_id: Option<Uuid>,
        #[serde(default)]
        total_cost_usd: f64,
        #[serde(default)]
        usage: Option<UsageInfo>,
        #[serde(default)]
        model_usage: serde_json::Value,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// Internal events emitted by the RA runtime
#[derive(Debug, Clone)]
pub enum AgentEvent {
    StateChanged {
        agent_id: Uuid,
        old: AgentState,
        new: AgentState,
    },
    MetricsUpdated {
        agent_id: Uuid,
        metrics: AgentMetrics,
    },
    StreamLine {
        agent_id: Uuid,
        event: StreamEvent,
    },
    ProcessExited {
        agent_id: Uuid,
        exit_code: i32,
    },
    CheckpointSaved {
        agent_id: Uuid,
        checkpoint_id: Uuid,
    },
    RateLimited {
        agent_id: Uuid,
        retry_after_ms: u64,
    },
    Error {
        agent_id: Uuid,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_system_event() {
        let json = r#"{"type":"system","subtype":"api_retry","attempt":1,"retry_delay_ms":2000}"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();
        match event {
            StreamEvent::System { subtype, .. } => {
                assert_eq!(subtype, "api_retry");
            }
            _ => panic!("Expected System event"),
        }
    }

    #[test]
    fn test_parse_result_event() {
        let json = r#"{
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "duration_ms": 5432,
            "num_turns": 1,
            "result": "Hello!",
            "session_id": "550e8400-e29b-41d4-a716-446655440000",
            "total_cost_usd": 0.0042,
            "usage": {
                "input_tokens": 100,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 50,
                "output_tokens": 25
            },
            "model_usage": {}
        }"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();
        match event {
            StreamEvent::Result {
                subtype,
                is_error,
                duration_ms,
                num_turns,
                result,
                total_cost_usd,
                usage,
                ..
            } => {
                assert_eq!(subtype, "success");
                assert!(!is_error);
                assert_eq!(duration_ms, 5432);
                assert_eq!(num_turns, 1);
                assert_eq!(result, "Hello!");
                assert!((total_cost_usd - 0.0042).abs() < f64::EPSILON);
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 100);
                assert_eq!(u.output_tokens, 25);
                assert_eq!(u.cache_read_input_tokens, 50);
            }
            _ => panic!("Expected Result event"),
        }
    }

    #[test]
    fn test_parse_assistant_event() {
        let json = r#"{"type":"assistant","message":{"content":"hi"}}"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, StreamEvent::Assistant { .. }));
    }
}
