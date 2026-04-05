use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub runtime: RuntimeSection,
    pub rate_limit: RateLimitConfig,
    pub persistence: PersistenceConfig,
    pub observability: ObservabilityConfig,
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSection {
    pub max_concurrency: usize,
    pub claude_binary: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub tokens_per_minute: u64,
    pub burst_multiplier: f64,
    pub backoff_base_ms: u64,
    pub backoff_max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    pub db_path: String,
    pub auto_checkpoint: bool,
    pub checkpoint_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub log_level: String,
    pub log_format: String,
    pub metrics_export_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub max_budget_usd: f64,
    pub token_budget: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            runtime: RuntimeSection {
                max_concurrency: 4,
                claude_binary: find_claude_binary(),
                default_model: "sonnet".to_string(),
            },
            rate_limit: RateLimitConfig {
                requests_per_minute: 50,
                tokens_per_minute: 400_000,
                burst_multiplier: 1.5,
                backoff_base_ms: 1000,
                backoff_max_ms: 60_000,
            },
            persistence: PersistenceConfig {
                db_path: "~/.ra/ra.db".to_string(),
                auto_checkpoint: true,
                checkpoint_interval_seconds: 60,
            },
            observability: ObservabilityConfig {
                log_level: "info".to_string(),
                log_format: "pretty".to_string(),
                metrics_export_path: "~/.ra/metrics/".to_string(),
            },
            defaults: DefaultsConfig {
                max_budget_usd: 5.0,
                token_budget: 200_000,
            },
        }
    }
}

impl RuntimeConfig {
    pub fn from_toml(content: &str) -> crate::error::RaResult<Self> {
        toml::from_str(content).map_err(|e| crate::error::RaError::Config(e.to_string()))
    }

    pub fn expand_path(path: &str) -> String {
        if path.starts_with("~/") {
            if let Some(home) = dirs_home() {
                return format!("{}{}", home, &path[1..]);
            }
        }
        path.to_string()
    }
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE")) // Windows fallback
        .ok()
}

/// Auto-detect the claude binary location
fn find_claude_binary() -> String {
    // 1. Check CLAUDE_BINARY env var
    if let Ok(path) = std::env::var("CLAUDE_BINARY") {
        return path;
    }

    // 2. Try `which claude` equivalent — search PATH
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path_var.split(sep) {
        let candidate = std::path::Path::new(dir).join("claude");
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    // 3. Check common installation locations
    let common_paths = [
        "/opt/homebrew/bin/claude", // macOS ARM (Homebrew)
        "/usr/local/bin/claude",    // macOS Intel / Linux
        "/usr/bin/claude",          // Linux system
    ];
    for path in &common_paths {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }

    // 4. Fallback — assume it's in PATH
    "claude".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.runtime.max_concurrency, 4);
        assert_eq!(config.runtime.default_model, "sonnet");
        assert_eq!(config.defaults.token_budget, 200_000);
        // claude_binary should be auto-detected, not empty
        assert!(!config.runtime.claude_binary.is_empty());
    }

    #[test]
    fn test_from_toml() {
        let toml = r#"
[runtime]
max_concurrency = 8
claude_binary = "/usr/local/bin/claude"
default_model = "opus"

[rate_limit]
requests_per_minute = 30
tokens_per_minute = 200000
burst_multiplier = 1.2
backoff_base_ms = 2000
backoff_max_ms = 120000

[persistence]
db_path = "~/.ra/ra.db"
auto_checkpoint = true
checkpoint_interval_seconds = 30

[observability]
log_level = "debug"
log_format = "json"
metrics_export_path = "~/.ra/metrics/"

[defaults]
max_budget_usd = 10.0
token_budget = 500000
"#;
        let config = RuntimeConfig::from_toml(toml).unwrap();
        assert_eq!(config.runtime.max_concurrency, 8);
        assert_eq!(config.runtime.default_model, "opus");
        assert_eq!(config.defaults.token_budget, 500_000);
    }

    #[test]
    fn test_expand_path() {
        let expanded = RuntimeConfig::expand_path("~/.ra/ra.db");
        assert!(!expanded.starts_with('~'));
    }
}
