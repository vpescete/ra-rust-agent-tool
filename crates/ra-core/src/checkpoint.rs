use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::metrics::WorkflowMetrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub workflow_state: WorkflowState,
    pub step_states: HashMap<String, StepState>,
    pub agent_outputs: HashMap<String, String>,
    pub variables: HashMap<String, String>,
    pub metrics: WorkflowMetrics,
}

impl Checkpoint {
    pub fn new(workflow_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            workflow_id,
            created_at: Utc::now(),
            workflow_state: WorkflowState::Running,
            step_states: HashMap::new(),
            agent_outputs: HashMap::new(),
            variables: HashMap::new(),
            metrics: WorkflowMetrics::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowState {
    Running,
    Paused,
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepState {
    Pending,
    Running,
    Completed { output: String },
    Failed { error: String, attempts: u32 },
    Skipped,
}

impl StepState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            StepState::Completed { .. } | StepState::Failed { .. } | StepState::Skipped
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, StepState::Completed { .. })
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, StepState::Failed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_new() {
        let wf_id = Uuid::new_v4();
        let cp = Checkpoint::new(wf_id);
        assert_eq!(cp.workflow_id, wf_id);
        assert_eq!(cp.workflow_state, WorkflowState::Running);
        assert!(cp.step_states.is_empty());
    }

    #[test]
    fn test_step_state_terminal() {
        assert!(!StepState::Pending.is_terminal());
        assert!(!StepState::Running.is_terminal());
        assert!(StepState::Completed {
            output: "ok".into()
        }
        .is_terminal());
        assert!(StepState::Failed {
            error: "err".into(),
            attempts: 1
        }
        .is_terminal());
        assert!(StepState::Skipped.is_terminal());
    }

    #[test]
    fn test_checkpoint_serialization() {
        let cp = Checkpoint::new(Uuid::new_v4());
        let json = serde_json::to_string(&cp).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(cp.id, deserialized.id);
        assert_eq!(cp.workflow_id, deserialized.workflow_id);
    }
}
