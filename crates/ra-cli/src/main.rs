mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ra",
    version,
    about = "RA - Rust Agent: Claude Code Orchestrator"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to config file
    #[arg(long, default_value = "~/.ra/config.toml")]
    config: String,

    /// Log level override
    #[arg(long)]
    log_level: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a workflow from a YAML definition
    Run {
        /// Path to workflow YAML file
        workflow: String,

        /// Resume from a specific checkpoint
        #[arg(long)]
        from_checkpoint: Option<String>,
    },

    /// Run a single agent with a prompt
    Agent {
        /// The prompt to send to Claude
        prompt: String,

        /// Model to use (e.g., sonnet, opus, haiku)
        #[arg(long)]
        model: Option<String>,

        /// Allowed tools (comma-separated)
        #[arg(long, value_delimiter = ',')]
        tools: Option<Vec<String>>,

        /// Maximum budget in USD
        #[arg(long)]
        budget: Option<f64>,

        /// Working directory for the agent
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Show status of running agents
    Status,

    /// Resume a workflow from a checkpoint
    Resume {
        /// Checkpoint ID
        checkpoint_id: String,
    },

    /// Show execution history
    History {
        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Launch TUI dashboard
    Dashboard,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = load_config(&cli.config)?;

    // Initialize tracing
    ra_observe::tracing_setup::init(&config.observability);

    match cli.command {
        Commands::Agent {
            prompt,
            model,
            tools,
            budget,
            cwd,
        } => {
            commands::agent::execute(prompt, model, tools, budget, cwd, &config).await?;
        }
        Commands::Run {
            workflow,
            from_checkpoint,
        } => {
            commands::run::execute(&workflow, from_checkpoint, &config).await?;
        }
        Commands::History { limit } => {
            commands::history::execute(limit, &config).await?;
        }
        Commands::Status => {
            commands::status::execute(&config).await?;
        }
        Commands::Resume { checkpoint_id } => {
            commands::resume::execute(&checkpoint_id, &config).await?;
        }
        Commands::Dashboard => {
            commands::dashboard::execute(&config).await?;
        }
    }

    Ok(())
}

fn load_config(path: &str) -> anyhow::Result<ra_core::RuntimeConfig> {
    let expanded = ra_core::config::RuntimeConfig::expand_path(path);
    if std::path::Path::new(&expanded).exists() {
        let content = std::fs::read_to_string(&expanded)?;
        Ok(ra_core::RuntimeConfig::from_toml(&content)?)
    } else {
        Ok(ra_core::RuntimeConfig::default())
    }
}
