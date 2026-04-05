use ra_core::{AgentConfig, AgentPriority, RuntimeConfig};
use ra_engine::run_single_agent;

pub async fn execute(
    prompt: String,
    model: Option<String>,
    tools: Option<Vec<String>>,
    budget: Option<f64>,
    cwd: Option<String>,
    config: &RuntimeConfig,
) -> anyhow::Result<()> {
    let agent_config = AgentConfig {
        model,
        allowed_tools: tools.unwrap_or_default(),
        max_budget_usd: budget,
        working_directory: cwd,
        priority: AgentPriority::Normal,
        ..Default::default()
    };

    eprintln!("Running agent...");

    let result = run_single_agent(config, prompt, agent_config, None).await?;

    // Print output
    println!("{}", result.output);

    // Print metrics to stderr
    eprintln!("---");
    eprintln!(
        "Tokens: {} in / {} out (total: {})",
        result.metrics.input_tokens,
        result.metrics.output_tokens,
        result.metrics.input_tokens + result.metrics.output_tokens
    );
    eprintln!("Cost: ${:.4}", result.total_cost_usd);
    eprintln!("Duration: {}ms", result.duration_ms);
    eprintln!("Turns: {}", result.num_turns);

    Ok(())
}
