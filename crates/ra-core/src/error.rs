use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum RaError {
    #[error("Failed to spawn claude process: {0}")]
    ProcessSpawn(String),

    #[error("Process wait error: {0}")]
    ProcessWait(String),

    #[error("Claude CLI exited with code {code}: {message}")]
    ClaudeError { code: i32, message: String },

    #[error("Workflow validation error: {0}")]
    WorkflowValidation(String),

    #[error("Cycle detected in workflow DAG")]
    DagCycle,

    #[error("Step '{0}' not found")]
    StepNotFound(String),

    #[error("Dependency '{dep}' for step '{step}' not found")]
    DependencyNotFound { step: String, dep: String },

    #[error("Token budget exceeded for agent {agent_id}")]
    TokenBudgetExceeded { agent_id: Uuid },

    #[error("Cost budget exceeded: ${spent:.4} >= ${limit:.4}")]
    CostBudgetExceeded { spent: f64, limit: f64 },

    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("Database error: {0}")]
    Database(String),

    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(Uuid),

    #[error("Run not found: {0}")]
    RunNotFound(Uuid),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Missing required template parameter: {0}")]
    MissingTemplateParameter(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type RaResult<T> = Result<T, RaError>;
