use std::sync::Arc;

use tokio::sync::broadcast;
use uuid::Uuid;

use ra_core::checkpoint::Checkpoint;
use ra_core::config::RuntimeConfig;
use ra_core::error::{RaError, RaResult};
use ra_core::event::AgentEvent;
use ra_core::workflow::Workflow;
use ra_core::CheckpointStore;

use crate::agent_manager::AgentManager;
use crate::context::ContextManager;
use crate::dag::{self, WorkflowResult};
use crate::scheduler::PriorityScheduler;

/// High-level workflow runner that ties everything together
pub struct WorkflowRunner {
    agent_manager: Arc<AgentManager>,
    scheduler: Arc<PriorityScheduler>,
    context_manager: Arc<ContextManager>,
    event_tx: broadcast::Sender<AgentEvent>,
    checkpoint_store: Option<Arc<dyn CheckpointStore>>,
}

impl WorkflowRunner {
    pub fn new(
        config: &RuntimeConfig,
        event_tx: broadcast::Sender<AgentEvent>,
        checkpoint_store: Option<Arc<dyn CheckpointStore>>,
    ) -> Self {
        let agent_manager = Arc::new(AgentManager::new(
            config.runtime.claude_binary.clone(),
            event_tx.clone(),
        ));
        let scheduler = Arc::new(PriorityScheduler::new(
            config.runtime.max_concurrency,
            &config.rate_limit,
        ));
        let context_manager = Arc::new(ContextManager::new());

        Self {
            agent_manager,
            scheduler,
            context_manager,
            event_tx,
            checkpoint_store,
        }
    }

    /// Load and execute a workflow from a YAML string
    pub async fn run_yaml(&self, yaml_content: &str) -> RaResult<WorkflowResult> {
        let workflow: Workflow =
            serde_yaml::from_str(yaml_content).map_err(|e| RaError::Yaml(e.to_string()))?;
        self.run_workflow(&workflow, None).await
    }

    /// Load and execute a workflow from a YAML file
    pub async fn run_file(&self, path: &str) -> RaResult<WorkflowResult> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| RaError::Config(format!("Failed to read workflow file: {}", e)))?;
        self.run_yaml(&content).await
    }

    /// Execute a workflow with optional initial checkpoint
    pub async fn run_workflow(
        &self,
        workflow: &Workflow,
        initial_checkpoint: Option<&Checkpoint>,
    ) -> RaResult<WorkflowResult> {
        // Validate DAG
        workflow.validate()?;

        // Execute
        let result = dag::execute(
            workflow,
            &self.agent_manager,
            &self.scheduler,
            &self.context_manager,
            &self.event_tx,
            initial_checkpoint,
        )
        .await?;

        // Save final checkpoint
        if let Some(ref store) = self.checkpoint_store {
            store.save(&result.checkpoint).await?;
        }

        Ok(result)
    }

    /// Resume a workflow from a checkpoint
    pub async fn resume(
        &self,
        checkpoint_id: Uuid,
        workflow: &Workflow,
    ) -> RaResult<WorkflowResult> {
        let store = self
            .checkpoint_store
            .as_ref()
            .ok_or_else(|| RaError::Config("No checkpoint store configured".to_string()))?;

        let checkpoint = store
            .load(checkpoint_id)
            .await?
            .ok_or(RaError::CheckpointNotFound(checkpoint_id))?;

        self.run_workflow(workflow, Some(&checkpoint)).await
    }

    /// Get the agent manager reference
    pub fn agent_manager(&self) -> &AgentManager {
        &self.agent_manager
    }

    /// Kill all running agents
    pub async fn kill_all(&self) {
        self.agent_manager.kill_all().await;
    }
}

/// Convenience: run a single agent (no workflow)
/// If `global_event_tx` is provided, events are forwarded to it (for IPC broadcasting).
pub async fn run_single_agent(
    config: &RuntimeConfig,
    prompt: String,
    agent_config: ra_core::AgentConfig,
    global_event_tx: Option<broadcast::Sender<ra_core::AgentEvent>>,
) -> RaResult<ra_core::AgentResult> {
    let (event_tx, _) = broadcast::channel(256);
    let agent_manager = AgentManager::new(config.runtime.claude_binary.clone(), event_tx);

    let agent = ra_core::Agent::new("single", prompt, agent_config);
    let mut rx = agent_manager.spawn_agent(agent).await?;

    let mut result_output = String::new();
    let mut final_result: Option<ra_core::AgentResult> = None;

    while let Some(event) = rx.recv().await {
        // Forward to global channel if provided
        if let Some(ref gtx) = global_event_tx {
            let _ = gtx.send(event.clone());
        }

        match &event {
            AgentEvent::StreamLine {
                event:
                    ra_core::StreamEvent::Result {
                        result,
                        is_error,
                        duration_ms,
                        num_turns,
                        total_cost_usd,
                        session_id,
                        usage,
                        ..
                    },
                ..
            } => {
                result_output = result.clone();
                let metrics = ra_core::AgentMetrics {
                    duration_ms: *duration_ms,
                    turns: *num_turns,
                    total_cost_usd: *total_cost_usd,
                    input_tokens: usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                    output_tokens: usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                    cache_read_tokens: usage
                        .as_ref()
                        .map(|u| u.cache_read_input_tokens)
                        .unwrap_or(0),
                    cache_creation_tokens: usage
                        .as_ref()
                        .map(|u| u.cache_creation_input_tokens)
                        .unwrap_or(0),
                    ..Default::default()
                };
                final_result = Some(ra_core::AgentResult {
                    output: result.clone(),
                    exit_code: if *is_error { 1 } else { 0 },
                    session_id: *session_id,
                    total_cost_usd: *total_cost_usd,
                    duration_ms: *duration_ms,
                    num_turns: *num_turns,
                    stop_reason: "end_turn".to_string(),
                    metrics,
                });
            }
            AgentEvent::Error { error, .. } => {
                result_output = error.clone();
            }
            _ => {}
        }
    }

    final_result.ok_or_else(|| RaError::ClaudeError {
        code: 1,
        message: if result_output.is_empty() {
            "No result received from Claude".to_string()
        } else {
            result_output
        },
    })
}
