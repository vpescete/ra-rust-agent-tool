use std::sync::Arc;

use tokio::sync::broadcast;

use ra_core::checkpoint::WorkflowState;
use ra_core::event::AgentEvent;
use ra_core::RuntimeConfig;
use ra_engine::WorkflowRunner;
use ra_store::{Database, SqliteCheckpointStore};

pub async fn execute(
    workflow_path: &str,
    _from_checkpoint: Option<String>,
    config: &RuntimeConfig,
) -> anyhow::Result<()> {
    let db_path = ra_core::config::RuntimeConfig::expand_path(&config.persistence.db_path);
    let db = Database::open(&db_path)?;
    let checkpoint_store = Arc::new(SqliteCheckpointStore::new(db.conn.clone()));

    let (event_tx, mut event_rx) = broadcast::channel::<AgentEvent>(256);

    let runner = WorkflowRunner::new(config, event_tx, Some(checkpoint_store));

    // Spawn event printer in background
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match &event {
                AgentEvent::StateChanged { agent_id, new, .. } => {
                    eprintln!("[{}] State: {:?}", &agent_id.to_string()[..8], new);
                }
                AgentEvent::Error { agent_id, error } => {
                    eprintln!("[{}] Error: {}", &agent_id.to_string()[..8], error);
                }
                AgentEvent::RateLimited { retry_after_ms, .. } => {
                    eprintln!("Rate limited, retrying in {}ms", retry_after_ms);
                }
                _ => {}
            }
        }
    });

    eprintln!("Running workflow: {}", workflow_path);

    let result = runner.run_file(workflow_path).await?;

    // Print results
    println!("\n=== Workflow Complete ===");
    println!("State: {:?}", result.state);
    println!(
        "Agents: {}/{} completed",
        result.metrics.completed_agents, result.metrics.total_agents
    );
    println!("Total cost: ${:.4}", result.metrics.total_cost_usd);
    println!(
        "Total tokens: {} in / {} out",
        result.metrics.total_input_tokens, result.metrics.total_output_tokens
    );

    if !result.step_outputs.is_empty() {
        println!("\n--- Step Outputs ---");
        for (key, value) in &result.step_outputs {
            println!("\n[{}]:", key);
            // Truncate long outputs
            if value.len() > 500 {
                println!("{}...", &value[..500]);
            } else {
                println!("{}", value);
            }
        }
    }

    match result.state {
        WorkflowState::Completed => {}
        WorkflowState::Failed => {
            std::process::exit(1);
        }
        WorkflowState::Aborted => {
            std::process::exit(2);
        }
        _ => {}
    }

    Ok(())
}
