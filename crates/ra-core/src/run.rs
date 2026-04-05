use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::AgentState;
use crate::metrics::AgentMetrics;

/// Unique identifier for a tracked run
pub type RunId = Uuid;

/// Overall status of a run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    PartiallyCompleted,
}

/// Per-agent status within a run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunStatus {
    pub name: String,
    pub agent_id: Uuid,
    pub state: AgentState,
    pub output: Option<String>,
    pub error: Option<String>,
    pub metrics: AgentMetrics,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// State of an entire tracked run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: RunId,
    pub status: RunStatus,
    pub agents: HashMap<String, AgentRunStatus>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
}

impl RunState {
    pub fn new() -> Self {
        Self {
            run_id: Uuid::new_v4(),
            status: RunStatus::Running,
            agents: HashMap::new(),
            created_at: Utc::now(),
            completed_at: None,
            total_cost_usd: 0.0,
            total_tokens: 0,
        }
    }

    /// Count agents by state
    pub fn completed_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.state == AgentState::Completed)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.state == AgentState::Failed)
            .count()
    }

    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.state == AgentState::Running)
            .count()
    }

    /// Check if all agents are done and update status
    pub fn update_status(&mut self) {
        let all_done = self.agents.values().all(|a| a.state.is_terminal());
        if all_done && !self.agents.is_empty() {
            let any_failed = self.agents.values().any(|a| a.state == AgentState::Failed);
            let all_failed = self.agents.values().all(|a| a.state == AgentState::Failed);

            self.status = if all_failed {
                RunStatus::Failed
            } else if any_failed {
                RunStatus::PartiallyCompleted
            } else {
                RunStatus::Completed
            };
            self.completed_at = Some(Utc::now());

            // Aggregate metrics
            self.total_cost_usd = self.agents.values().map(|a| a.metrics.total_cost_usd).sum();
            self.total_tokens = self.agents.values().map(|a| a.metrics.total_tokens()).sum();
        }
    }
}

impl Default for RunState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_state_new() {
        let state = RunState::new();
        assert_eq!(state.status, RunStatus::Running);
        assert!(state.agents.is_empty());
    }

    #[test]
    fn test_update_status_all_completed() {
        let mut state = RunState::new();
        state.agents.insert(
            "a".to_string(),
            AgentRunStatus {
                name: "a".to_string(),
                agent_id: Uuid::new_v4(),
                state: AgentState::Completed,
                output: Some("done".to_string()),
                error: None,
                metrics: AgentMetrics::default(),
                started_at: None,
                completed_at: None,
            },
        );
        state.update_status();
        assert_eq!(state.status, RunStatus::Completed);
    }

    #[test]
    fn test_update_status_partial() {
        let mut state = RunState::new();
        state.agents.insert(
            "a".to_string(),
            AgentRunStatus {
                name: "a".to_string(),
                agent_id: Uuid::new_v4(),
                state: AgentState::Completed,
                output: None,
                error: None,
                metrics: AgentMetrics::default(),
                started_at: None,
                completed_at: None,
            },
        );
        state.agents.insert(
            "b".to_string(),
            AgentRunStatus {
                name: "b".to_string(),
                agent_id: Uuid::new_v4(),
                state: AgentState::Failed,
                output: None,
                error: Some("err".to_string()),
                metrics: AgentMetrics::default(),
                started_at: None,
                completed_at: None,
            },
        );
        state.update_status();
        assert_eq!(state.status, RunStatus::PartiallyCompleted);
    }

    #[test]
    fn test_counts() {
        let mut state = RunState::new();
        state.agents.insert(
            "a".to_string(),
            AgentRunStatus {
                name: "a".to_string(),
                agent_id: Uuid::new_v4(),
                state: AgentState::Completed,
                output: None,
                error: None,
                metrics: AgentMetrics::default(),
                started_at: None,
                completed_at: None,
            },
        );
        state.agents.insert(
            "b".to_string(),
            AgentRunStatus {
                name: "b".to_string(),
                agent_id: Uuid::new_v4(),
                state: AgentState::Running,
                output: None,
                error: None,
                metrics: AgentMetrics::default(),
                started_at: None,
                completed_at: None,
            },
        );
        assert_eq!(state.completed_count(), 1);
        assert_eq!(state.running_count(), 1);
        assert_eq!(state.failed_count(), 0);
    }
}
