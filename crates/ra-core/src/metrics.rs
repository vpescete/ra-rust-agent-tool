use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub api_calls: u32,
    pub tool_calls: u32,
    pub retries: u32,
    pub turns: u32,
    pub duration_ms: u64,
    pub errors: Vec<String>,
}

impl AgentMetrics {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMetrics {
    pub total_agents: u32,
    pub completed_agents: u32,
    pub failed_agents: u32,
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub elapsed_ms: u64,
}

impl WorkflowMetrics {
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_agents == 0 {
            return 0.0;
        }
        self.completed_agents as f64 / self.total_agents as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_metrics_total() {
        let m = AgentMetrics {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        assert_eq!(m.total_tokens(), 150);
    }

    #[test]
    fn test_workflow_success_rate() {
        let m = WorkflowMetrics {
            total_agents: 4,
            completed_agents: 3,
            failed_agents: 1,
            ..Default::default()
        };
        assert!((m.success_rate() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_workflow_success_rate_zero() {
        let m = WorkflowMetrics::default();
        assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
    }
}
