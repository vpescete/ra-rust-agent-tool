use std::path::Path;

use ra_core::error::{RaError, RaResult};

use crate::metrics_collector::DashboardData;

/// Export metrics data as JSON
pub fn export_json(data: &DashboardData, path: &Path) -> RaResult<()> {
    let json = serde_json::json!({
        "workflow": {
            "total_agents": data.workflow.total_agents,
            "completed_agents": data.workflow.completed_agents,
            "failed_agents": data.workflow.failed_agents,
            "total_cost_usd": data.workflow.total_cost_usd,
            "total_input_tokens": data.workflow.total_input_tokens,
            "total_output_tokens": data.workflow.total_output_tokens,
            "elapsed_ms": data.workflow.elapsed_ms,
        },
        "agents": data.agents.iter().map(|(id, m)| {
            serde_json::json!({
                "id": id.to_string(),
                "input_tokens": m.input_tokens,
                "output_tokens": m.output_tokens,
                "total_cost_usd": m.total_cost_usd,
                "duration_ms": m.duration_ms,
                "turns": m.turns,
                "retries": m.retries,
                "errors": m.errors,
            })
        }).collect::<Vec<_>>(),
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&json)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Export metrics data as CSV
pub fn export_csv(data: &DashboardData, path: &Path) -> RaResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut writer = csv::Writer::from_path(path)
        .map_err(|e| RaError::Config(format!("Failed to create CSV writer: {}", e)))?;

    writer
        .write_record([
            "agent_id",
            "input_tokens",
            "output_tokens",
            "cost_usd",
            "duration_ms",
            "turns",
            "retries",
        ])
        .map_err(|e| RaError::Config(e.to_string()))?;

    for (id, m) in &data.agents {
        writer
            .write_record([
                &id.to_string(),
                &m.input_tokens.to_string(),
                &m.output_tokens.to_string(),
                &format!("{:.4}", m.total_cost_usd),
                &m.duration_ms.to_string(),
                &m.turns.to_string(),
                &m.retries.to_string(),
            ])
            .map_err(|e| RaError::Config(e.to_string()))?;
    }

    writer.flush().map_err(|e| RaError::Config(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::metrics::{AgentMetrics, WorkflowMetrics};
    use uuid::Uuid;

    fn sample_data() -> DashboardData {
        DashboardData {
            agents: vec![
                (
                    Uuid::new_v4(),
                    AgentMetrics {
                        input_tokens: 1000,
                        output_tokens: 500,
                        total_cost_usd: 0.05,
                        duration_ms: 3000,
                        turns: 2,
                        ..Default::default()
                    },
                ),
                (
                    Uuid::new_v4(),
                    AgentMetrics {
                        input_tokens: 2000,
                        output_tokens: 800,
                        total_cost_usd: 0.08,
                        duration_ms: 5000,
                        turns: 3,
                        ..Default::default()
                    },
                ),
            ],
            workflow: WorkflowMetrics {
                total_agents: 2,
                completed_agents: 2,
                ..Default::default()
            },
            recent_events: vec![],
        }
    }

    #[test]
    fn test_export_json() {
        let data = sample_data();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.json");

        export_json(&data, &path).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["agents"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_export_csv() {
        let data = sample_data();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.csv");

        export_csv(&data, &path).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
    }
}
