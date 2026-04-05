use std::collections::HashMap;

use tokio::sync::broadcast;
use uuid::Uuid;

use ra_core::agent::{Agent, AgentConfig, AgentResult};
use ra_core::checkpoint::{Checkpoint, StepState, WorkflowState};
use ra_core::error::{RaError, RaResult};
use ra_core::event::{AgentEvent, StreamEvent};
use ra_core::metrics::WorkflowMetrics;
use ra_core::workflow::{DependencyCondition, FailureAction, Step, Workflow};

use crate::agent_manager::AgentManager;
use crate::context::ContextManager;
use crate::scheduler::PriorityScheduler;

/// Result of a complete workflow execution
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub workflow_id: Uuid,
    pub state: WorkflowState,
    pub step_outputs: HashMap<String, String>,
    pub metrics: WorkflowMetrics,
    pub checkpoint: Checkpoint,
}

/// Execute a workflow DAG
pub async fn execute(
    workflow: &Workflow,
    agent_manager: &AgentManager,
    scheduler: &PriorityScheduler,
    context_manager: &ContextManager,
    _event_tx: &broadcast::Sender<AgentEvent>,
    initial_checkpoint: Option<&Checkpoint>,
) -> RaResult<WorkflowResult> {
    // Initialize state from checkpoint or fresh
    let mut step_states: HashMap<String, StepState> = match initial_checkpoint {
        Some(cp) => cp.step_states.clone(),
        None => workflow
            .steps
            .iter()
            .map(|s| (s.id.clone(), StepState::Pending))
            .collect(),
    };

    let mut variables: HashMap<String, String> = match initial_checkpoint {
        Some(cp) => cp.variables.clone(),
        None => HashMap::new(),
    };

    let mut metrics = match initial_checkpoint {
        Some(cp) => cp.metrics.clone(),
        None => WorkflowMetrics {
            total_agents: workflow.steps.len() as u32,
            ..Default::default()
        },
    };

    // Main execution loop
    loop {
        // Find ready steps
        let ready = find_ready_steps(workflow, &step_states);

        if ready.is_empty() {
            // Check if all done
            let all_terminal = step_states.values().all(|s| s.is_terminal());
            if all_terminal {
                break;
            }
            // Check for deadlock (no ready steps but not all terminal)
            let any_running = step_states
                .values()
                .any(|s| matches!(s, StepState::Running));
            if !any_running {
                // Deadlock: nothing ready, nothing running, not all terminal
                return Err(RaError::WorkflowValidation(
                    "Workflow deadlocked: no steps can proceed".to_string(),
                ));
            }
            // Wait a bit for running steps to complete
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            continue;
        }

        // Execute ready steps (up to max_concurrency via scheduler)
        let mut handles = Vec::new();

        for step in &ready {
            // Acquire scheduler slot
            let _permit = scheduler.acquire(step.agent_config.priority).await?;

            // Merge shared context with local variables for resolution
            let shared_vars = context_manager.shared.snapshot().await;
            let mut all_vars = variables.clone();
            all_vars.extend(shared_vars);
            let resolved_prompt = resolve_variables(&step.prompt, &all_vars);

            // Create agent
            let agent_config = AgentConfig {
                model: step.agent_config.model.clone(),
                allowed_tools: step.agent_config.allowed_tools.clone(),
                max_budget_usd: step.agent_config.max_budget_usd,
                working_directory: step.agent_config.working_directory.clone(),
                system_prompt: step.agent_config.system_prompt.clone(),
                session_id: step.agent_config.session_id,
                mcp_config: step.agent_config.mcp_config.clone(),
                extra_args: step.agent_config.extra_args.clone(),
                priority: step.agent_config.priority,
                max_turns: step.agent_config.max_turns,
                token_budget: step.agent_config.token_budget,
            };

            let agent = Agent::new(&step.name, resolved_prompt, agent_config);

            // Register token budget
            if let Some(budget) = step.agent_config.token_budget {
                context_manager.register(agent.id, budget).await;
            }

            // Mark as running
            step_states.insert(step.id.clone(), StepState::Running);

            // Spawn agent
            let rx = agent_manager.spawn_agent(agent).await?;
            handles.push((step.id.clone(), step.output_var.clone(), rx));
        }

        // Collect results from spawned agents
        for (step_id, output_var, mut rx) in handles {
            let mut result_output = String::new();
            let mut agent_result: Option<AgentResult> = None;

            // Read all events until the channel closes
            while let Some(event) = rx.recv().await {
                match &event {
                    AgentEvent::StreamLine {
                        event:
                            StreamEvent::Result {
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
                        if !is_error {
                            result_output = result.clone();
                        }
                        let result_metrics = ra_core::metrics::AgentMetrics {
                            duration_ms: *duration_ms,
                            turns: *num_turns,
                            total_cost_usd: *total_cost_usd,
                            input_tokens: usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                            output_tokens: usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                            ..Default::default()
                        };
                        agent_result = Some(AgentResult {
                            output: result.clone(),
                            exit_code: if *is_error { 1 } else { 0 },
                            session_id: *session_id,
                            total_cost_usd: *total_cost_usd,
                            duration_ms: *duration_ms,
                            num_turns: *num_turns,
                            stop_reason: "end_turn".to_string(),
                            metrics: result_metrics,
                        });
                    }
                    AgentEvent::ProcessExited { exit_code, .. } => {
                        if *exit_code != 0 && agent_result.is_none() {
                            result_output = format!("Process exited with code {}", exit_code);
                        }
                    }
                    AgentEvent::Error { error, .. } => {
                        result_output = error.clone();
                    }
                    _ => {}
                }
            }

            // Determine step outcome
            let success = agent_result
                .as_ref()
                .map(|r| r.exit_code == 0)
                .unwrap_or(false);

            if success {
                // Store output variable in both local vars and shared context
                if let Some(ref var_name) = output_var {
                    variables.insert(var_name.clone(), result_output.clone());
                    context_manager
                        .shared
                        .set(var_name.clone(), result_output.clone())
                        .await;
                }

                step_states.insert(
                    step_id.clone(),
                    StepState::Completed {
                        output: result_output,
                    },
                );
                metrics.completed_agents += 1;

                if let Some(ref ar) = agent_result {
                    metrics.total_cost_usd += ar.total_cost_usd;
                    metrics.total_input_tokens += ar.metrics.input_tokens;
                    metrics.total_output_tokens += ar.metrics.output_tokens;
                }
            } else {
                // Handle failure
                let step = workflow.steps.iter().find(|s| s.id == step_id);
                let current_attempts = match step_states.get(&step_id) {
                    Some(StepState::Failed { attempts, .. }) => *attempts,
                    _ => 0,
                };

                let normalized_failure = step
                    .and_then(|s| s.on_failure.as_ref())
                    .map(|f| f.normalize());
                match normalized_failure.as_ref() {
                    Some(FailureAction::Retry { max_attempts })
                        if current_attempts < *max_attempts =>
                    {
                        // Retry: reset to Pending
                        step_states.insert(step_id.clone(), StepState::Pending);
                    }
                    Some(FailureAction::Skip) => {
                        step_states.insert(step_id.clone(), StepState::Skipped);
                        metrics.failed_agents += 1;
                    }
                    Some(FailureAction::Abort) => {
                        step_states.insert(
                            step_id.clone(),
                            StepState::Failed {
                                error: result_output,
                                attempts: current_attempts + 1,
                            },
                        );
                        metrics.failed_agents += 1;
                        // Abort entire workflow
                        let checkpoint = Checkpoint {
                            id: Uuid::new_v4(),
                            workflow_id: workflow.id,
                            created_at: chrono::Utc::now(),
                            workflow_state: WorkflowState::Aborted,
                            step_states,
                            agent_outputs: variables.clone(),
                            variables,
                            metrics: metrics.clone(),
                        };
                        return Ok(WorkflowResult {
                            workflow_id: workflow.id,
                            state: WorkflowState::Aborted,
                            step_outputs: checkpoint.agent_outputs.clone(),
                            metrics,
                            checkpoint,
                        });
                    }
                    Some(FailureAction::Fallback {
                        step_id: fallback_id,
                    }) => {
                        // Reset fallback step to Pending so it gets picked up
                        step_states.insert(fallback_id.clone(), StepState::Pending);
                        step_states.insert(
                            step_id.clone(),
                            StepState::Failed {
                                error: result_output,
                                attempts: current_attempts + 1,
                            },
                        );
                        metrics.failed_agents += 1;
                    }
                    _ => {
                        // No recovery policy, mark as failed
                        step_states.insert(
                            step_id.clone(),
                            StepState::Failed {
                                error: result_output,
                                attempts: current_attempts + 1,
                            },
                        );
                        metrics.failed_agents += 1;
                    }
                }
            }
        }
    }

    // Determine final workflow state
    let final_state = if step_states.values().all(|s| s.is_success()) {
        WorkflowState::Completed
    } else if step_states.values().any(|s| s.is_failure()) {
        WorkflowState::Failed
    } else {
        WorkflowState::Completed
    };

    let checkpoint = Checkpoint {
        id: Uuid::new_v4(),
        workflow_id: workflow.id,
        created_at: chrono::Utc::now(),
        workflow_state: final_state.clone(),
        step_states,
        agent_outputs: variables.clone(),
        variables: variables.clone(),
        metrics: metrics.clone(),
    };

    Ok(WorkflowResult {
        workflow_id: workflow.id,
        state: final_state,
        step_outputs: variables,
        metrics,
        checkpoint,
    })
}

/// Find steps whose dependencies are all satisfied
fn find_ready_steps<'a>(
    workflow: &'a Workflow,
    step_states: &HashMap<String, StepState>,
) -> Vec<&'a Step> {
    workflow
        .steps
        .iter()
        .filter(|step| {
            // Must be pending
            let is_pending = matches!(step_states.get(&step.id), Some(StepState::Pending) | None);
            if !is_pending {
                return false;
            }

            // All dependencies must be satisfied
            step.depends_on.iter().all(|dep| {
                let dep_state = step_states.get(&dep.step_id);
                match (&dep.condition, dep_state) {
                    (DependencyCondition::Success, Some(StepState::Completed { .. })) => true,
                    (DependencyCondition::Failure, Some(StepState::Failed { .. })) => true,
                    (DependencyCondition::Always, Some(s)) if s.is_terminal() => true,
                    _ => false,
                }
            })
        })
        .collect()
}

/// Replace {{variable}} placeholders in prompt with actual values
fn resolve_variables(prompt: &str, variables: &HashMap<String, String>) -> String {
    let mut result = prompt.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_variables() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("age".to_string(), "30".to_string());

        let prompt = "Hello {{name}}, you are {{age}} years old.";
        let result = resolve_variables(prompt, &vars);
        assert_eq!(result, "Hello Alice, you are 30 years old.");
    }

    #[test]
    fn test_resolve_no_vars() {
        let vars = HashMap::new();
        let prompt = "Hello world";
        assert_eq!(resolve_variables(prompt, &vars), "Hello world");
    }

    #[test]
    fn test_resolve_missing_var() {
        let vars = HashMap::new();
        let prompt = "Hello {{unknown}}";
        assert_eq!(resolve_variables(prompt, &vars), "Hello {{unknown}}");
    }

    #[test]
    fn test_find_ready_no_deps() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            steps: vec![Step {
                id: "a".to_string(),
                name: "a".to_string(),
                prompt: "do a".to_string(),
                agent_config: AgentConfig::default(),
                depends_on: vec![],
                on_failure: None,
                output_var: None,
                inject_context: false,
            }],
            config: Default::default(),
        };
        let states: HashMap<String, StepState> = [("a".to_string(), StepState::Pending)]
            .into_iter()
            .collect();

        let ready = find_ready_steps(&workflow, &states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");
    }

    #[test]
    fn test_find_ready_with_deps() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            steps: vec![
                Step {
                    id: "a".to_string(),
                    name: "a".to_string(),
                    prompt: "do a".to_string(),
                    agent_config: AgentConfig::default(),
                    depends_on: vec![],
                    on_failure: None,
                    output_var: None,
                    inject_context: false,
                },
                Step {
                    id: "b".to_string(),
                    name: "b".to_string(),
                    prompt: "do b".to_string(),
                    agent_config: AgentConfig::default(),
                    depends_on: vec![ra_core::workflow::StepDependency {
                        step_id: "a".to_string(),
                        condition: DependencyCondition::Success,
                    }],
                    on_failure: None,
                    output_var: None,
                    inject_context: false,
                },
            ],
            config: Default::default(),
        };

        // A pending, B pending -> only A ready
        let states: HashMap<String, StepState> = [
            ("a".to_string(), StepState::Pending),
            ("b".to_string(), StepState::Pending),
        ]
        .into_iter()
        .collect();
        let ready = find_ready_steps(&workflow, &states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");

        // A completed, B pending -> B ready
        let states2: HashMap<String, StepState> = [
            (
                "a".to_string(),
                StepState::Completed {
                    output: "done".into(),
                },
            ),
            ("b".to_string(), StepState::Pending),
        ]
        .into_iter()
        .collect();
        let ready2 = find_ready_steps(&workflow, &states2);
        assert_eq!(ready2.len(), 1);
        assert_eq!(ready2[0].id, "b");
    }
}
