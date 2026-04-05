use ra_core::event::{StreamEvent, UsageInfo};
use ra_core::metrics::AgentMetrics;

/// Parse a JSONL line from claude's stream-json output
pub fn parse_stream_line(line: &str) -> Option<StreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// Incrementally update metrics from a stream event
pub fn update_metrics(metrics: &mut AgentMetrics, event: &StreamEvent) {
    match event {
        StreamEvent::Result {
            usage,
            total_cost_usd,
            num_turns,
            duration_ms,
            is_error,
            ..
        } => {
            if let Some(u) = usage {
                metrics.input_tokens = u.input_tokens;
                metrics.output_tokens = u.output_tokens;
                metrics.cache_read_tokens = u.cache_read_input_tokens;
                metrics.cache_creation_tokens = u.cache_creation_input_tokens;
            }
            metrics.total_cost_usd = *total_cost_usd;
            metrics.turns = *num_turns;
            metrics.duration_ms = *duration_ms;
            if *is_error {
                metrics.errors.push("Claude returned an error".to_string());
            }
        }
        StreamEvent::System { subtype, .. } => {
            if subtype == "api_retry" {
                metrics.retries += 1;
            }
            metrics.api_calls += 1;
        }
        StreamEvent::Assistant { .. } => {}
    }
}

/// Detect if a stream event indicates rate limiting, returns retry delay in ms
pub fn is_rate_limited(event: &StreamEvent) -> Option<u64> {
    if let StreamEvent::System { subtype, data } = event {
        if subtype == "api_retry" {
            return data.get("retry_delay_ms").and_then(|v| v.as_u64());
        }
    }
    None
}

/// Extract the final result text from a Result event
pub fn extract_result(event: &StreamEvent) -> Option<String> {
    if let StreamEvent::Result {
        result, is_error, ..
    } = event
    {
        if !is_error {
            return Some(result.clone());
        }
    }
    None
}

/// Extract usage info from a Result event
pub fn extract_usage(event: &StreamEvent) -> Option<UsageInfo> {
    if let StreamEvent::Result { usage, .. } = event {
        return usage.clone();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_line() {
        assert!(parse_stream_line("").is_none());
        assert!(parse_stream_line("   ").is_none());
    }

    #[test]
    fn test_parse_invalid_json() {
        assert!(parse_stream_line("not json").is_none());
        assert!(parse_stream_line("{invalid}").is_none());
    }

    #[test]
    fn test_parse_system_event() {
        let line = r#"{"type":"system","subtype":"api_retry","retry_delay_ms":2000}"#;
        let event = parse_stream_line(line).unwrap();
        assert!(matches!(event, StreamEvent::System { .. }));
    }

    #[test]
    fn test_parse_result_event() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1234,"num_turns":1,"result":"Hello","total_cost_usd":0.01,"usage":{"input_tokens":50,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0},"model_usage":{}}"#;
        let event = parse_stream_line(line).unwrap();
        match &event {
            StreamEvent::Result {
                result,
                total_cost_usd,
                ..
            } => {
                assert_eq!(result, "Hello");
                assert!(*total_cost_usd > 0.0);
            }
            _ => panic!("Expected Result"),
        }
    }

    #[test]
    fn test_update_metrics_from_result() {
        let event = StreamEvent::Result {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: 5000,
            num_turns: 3,
            result: "done".to_string(),
            session_id: None,
            total_cost_usd: 0.05,
            usage: Some(UsageInfo {
                input_tokens: 1000,
                output_tokens: 500,
                cache_creation_input_tokens: 100,
                cache_read_input_tokens: 200,
            }),
            model_usage: serde_json::Value::Null,
        };

        let mut metrics = AgentMetrics::default();
        update_metrics(&mut metrics, &event);

        assert_eq!(metrics.input_tokens, 1000);
        assert_eq!(metrics.output_tokens, 500);
        assert_eq!(metrics.turns, 3);
        assert_eq!(metrics.duration_ms, 5000);
    }

    #[test]
    fn test_is_rate_limited() {
        let event = StreamEvent::System {
            subtype: "api_retry".to_string(),
            data: serde_json::json!({"retry_delay_ms": 3000}),
        };
        assert_eq!(is_rate_limited(&event), Some(3000));

        let event2 = StreamEvent::System {
            subtype: "other".to_string(),
            data: serde_json::Value::Null,
        };
        assert_eq!(is_rate_limited(&event2), None);
    }

    #[test]
    fn test_extract_result() {
        let event = StreamEvent::Result {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: 100,
            num_turns: 1,
            result: "Hello World".to_string(),
            session_id: None,
            total_cost_usd: 0.001,
            usage: None,
            model_usage: serde_json::Value::Null,
        };
        assert_eq!(extract_result(&event), Some("Hello World".to_string()));
    }
}
