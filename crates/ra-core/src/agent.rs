use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::metrics::AgentMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Killed,
}

impl AgentState {
    pub fn can_transition_to(&self, target: AgentState) -> bool {
        use AgentState::*;
        matches!(
            (self, target),
            (Pending, Running)
                | (Running, Paused)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Killed)
                | (Paused, Running)
                | (Paused, Killed)
                | (Failed, Pending) // retry
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            AgentState::Completed | AgentState::Failed | AgentState::Killed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum AgentPriority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub model: Option<String>,
    pub allowed_tools: Vec<String>,
    pub max_budget_usd: Option<f64>,
    pub working_directory: Option<String>,
    pub system_prompt: Option<String>,
    pub session_id: Option<Uuid>,
    pub mcp_config: Option<String>,
    pub extra_args: Vec<String>,
    pub priority: AgentPriority,
    pub max_turns: Option<u32>,
    pub token_budget: Option<u64>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: None,
            allowed_tools: Vec::new(),
            max_budget_usd: None,
            working_directory: None,
            system_prompt: None,
            session_id: None,
            mcp_config: None,
            extra_args: Vec::new(),
            priority: AgentPriority::Normal,
            max_turns: None,
            token_budget: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub output: String,
    pub exit_code: i32,
    pub session_id: Option<Uuid>,
    pub total_cost_usd: f64,
    pub duration_ms: u64,
    pub num_turns: u32,
    pub stop_reason: String,
    pub metrics: AgentMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub prompt: String,
    pub config: AgentConfig,
    pub state: AgentState,
    pub metrics: AgentMetrics,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<AgentResult>,
    pub workflow_id: Option<Uuid>,
    pub step_id: Option<String>,
}

impl Agent {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>, config: AgentConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            prompt: prompt.into(),
            config,
            state: AgentState::Pending,
            metrics: AgentMetrics::default(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            workflow_id: None,
            step_id: None,
        }
    }

    pub fn transition_to(&mut self, new_state: AgentState) -> Result<(), String> {
        if self.state.can_transition_to(new_state) {
            self.state = new_state;
            match new_state {
                AgentState::Running => self.started_at = Some(Utc::now()),
                s if s.is_terminal() => self.completed_at = Some(Utc::now()),
                _ => {}
            }
            Ok(())
        } else {
            Err(format!(
                "Invalid state transition: {:?} -> {:?}",
                self.state, new_state
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        assert!(AgentState::Pending.can_transition_to(AgentState::Running));
        assert!(AgentState::Running.can_transition_to(AgentState::Completed));
        assert!(AgentState::Running.can_transition_to(AgentState::Failed));
        assert!(AgentState::Running.can_transition_to(AgentState::Killed));
        assert!(AgentState::Running.can_transition_to(AgentState::Paused));
        assert!(AgentState::Paused.can_transition_to(AgentState::Running));
        assert!(AgentState::Paused.can_transition_to(AgentState::Killed));
        assert!(AgentState::Failed.can_transition_to(AgentState::Pending)); // retry
    }

    #[test]
    fn test_invalid_transitions() {
        assert!(!AgentState::Pending.can_transition_to(AgentState::Completed));
        assert!(!AgentState::Completed.can_transition_to(AgentState::Running));
        assert!(!AgentState::Killed.can_transition_to(AgentState::Running));
        assert!(!AgentState::Failed.can_transition_to(AgentState::Running));
    }

    #[test]
    fn test_is_terminal() {
        assert!(AgentState::Completed.is_terminal());
        assert!(AgentState::Failed.is_terminal());
        assert!(AgentState::Killed.is_terminal());
        assert!(!AgentState::Pending.is_terminal());
        assert!(!AgentState::Running.is_terminal());
        assert!(!AgentState::Paused.is_terminal());
    }

    #[test]
    fn test_agent_transition() {
        let mut agent = Agent::new("test", "hello", AgentConfig::default());
        assert_eq!(agent.state, AgentState::Pending);
        assert!(agent.started_at.is_none());

        agent.transition_to(AgentState::Running).unwrap();
        assert_eq!(agent.state, AgentState::Running);
        assert!(agent.started_at.is_some());

        agent.transition_to(AgentState::Completed).unwrap();
        assert_eq!(agent.state, AgentState::Completed);
        assert!(agent.completed_at.is_some());
    }

    #[test]
    fn test_agent_invalid_transition() {
        let mut agent = Agent::new("test", "hello", AgentConfig::default());
        assert!(agent.transition_to(AgentState::Completed).is_err());
    }
}
