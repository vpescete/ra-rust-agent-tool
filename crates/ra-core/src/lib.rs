pub mod agent;
pub mod checkpoint;
pub mod config;
pub mod error;
pub mod event;
pub mod ipc;
pub mod metrics;
pub mod run;
pub mod template;
pub mod workflow;

// Re-exports
pub use agent::{Agent, AgentConfig, AgentPriority, AgentResult, AgentState};
pub use checkpoint::{Checkpoint, StepState, WorkflowState};
pub use config::RuntimeConfig;
pub use error::{RaError, RaResult};
pub use event::{AgentEvent, StreamEvent, UsageInfo};
pub use metrics::{AgentMetrics, WorkflowMetrics};
pub use run::{AgentRunStatus, RunId, RunState, RunStatus};
pub use template::{Template, TemplateInfo, TemplateParameter};
pub use workflow::{
    DependencyCondition, FailureAction, Step, StepDependency, Workflow, WorkflowConfig,
};

use async_trait::async_trait;
use uuid::Uuid;

/// Executor trait — wraps Claude CLI subprocess. Mockable for testing.
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn spawn(&self, agent: &Agent) -> RaResult<AgentHandle>;

    async fn kill(&self, agent_id: Uuid) -> RaResult<()>;
}

/// Handle to a running agent process
pub struct AgentHandle {
    pub agent_id: Uuid,
    pub event_rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
}

/// Persistence backend trait
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    async fn save(&self, checkpoint: &Checkpoint) -> RaResult<Uuid>;
    async fn load(&self, id: Uuid) -> RaResult<Option<Checkpoint>>;
    async fn list(&self, workflow_id: Uuid) -> RaResult<Vec<Checkpoint>>;
    async fn latest(&self, workflow_id: Uuid) -> RaResult<Option<Checkpoint>>;
}

/// Event sink for observability
pub trait EventSink: Send + Sync {
    fn send(&self, event: AgentEvent);
}

/// Scheduler / rate limiter trait
#[async_trait]
pub trait Scheduler: Send + Sync {
    async fn acquire(&self, priority: AgentPriority) -> RaResult<SchedulerPermit>;
    async fn notify_rate_limited(&self, retry_after_ms: u64);
    async fn release(&self);
}

/// RAII permit — represents an acquired scheduling slot
pub struct SchedulerPermit {
    _private: (),
}

impl SchedulerPermit {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for SchedulerPermit {
    fn default() -> Self {
        Self::new()
    }
}
