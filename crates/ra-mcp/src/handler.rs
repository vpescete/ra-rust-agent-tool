//! Tool execution handler — bridges MCP calls to ra-engine

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use ra_core::agent::{AgentConfig, AgentPriority, AgentState};
use ra_core::config::RuntimeConfig;
use ra_core::event::AgentEvent;
use ra_core::metrics::AgentMetrics;
use ra_core::run::{AgentRunStatus, RunId, RunState, RunStatus};
use ra_core::template::{Template, TemplateInfo, TemplateParameter, TemplateSource};
use ra_engine::runner::{run_single_agent, WorkflowRunner};
use ra_store::{Database, HistoryStore, SqliteCheckpointStore};

use crate::protocol::ToolContent;

pub struct Handler {
    config: RuntimeConfig,
    db: Option<Database>,
    active_runs: Arc<RwLock<HashMap<RunId, RunState>>>,
    /// Global event sender — events are forwarded to IPC broadcaster
    global_event_tx: broadcast::Sender<AgentEvent>,
}

impl Handler {
    pub fn new(config: RuntimeConfig, global_event_tx: broadcast::Sender<AgentEvent>) -> Self {
        let db_path = RuntimeConfig::expand_path(&config.persistence.db_path);
        let db = Database::open(&db_path).ok();

        Self {
            config,
            db,
            active_runs: Arc::new(RwLock::new(HashMap::new())),
            global_event_tx,
        }
    }

    // =====================================================================
    // Helper: parse agent definitions from JSON params
    // =====================================================================

    #[allow(clippy::type_complexity)]
    fn parse_agent_defs(
        params: &serde_json::Value,
    ) -> Result<
        Vec<(
            String,
            String,
            Option<String>,
            Vec<String>,
            Option<f64>,
            Option<String>,
        )>,
        String,
    > {
        let agents = params
            .get("agents")
            .and_then(|a| a.as_array())
            .ok_or("Missing 'agents' array")?;

        let mut defs = Vec::new();
        for agent_def in agents {
            let name = agent_def
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed")
                .to_string();
            let prompt = agent_def
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or(format!("Agent '{}' missing 'prompt'", name))?
                .to_string();
            let model = agent_def
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let allowed_tools: Vec<String> = agent_def
                .get("allowed_tools")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let max_budget = agent_def.get("max_budget_usd").and_then(|v| v.as_f64());
            let cwd = agent_def
                .get("working_directory")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            defs.push((name, prompt, model, allowed_tools, max_budget, cwd));
        }
        Ok(defs)
    }

    // =====================================================================
    // V1: Synchronous ra_run_agents (blocking)
    // =====================================================================

    pub async fn run_agents(&self, params: serde_json::Value) -> Result<Vec<ToolContent>, String> {
        let defs = Self::parse_agent_defs(&params)?;
        let max_concurrency = params
            .get("max_concurrency")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.runtime.max_concurrency as u64)
            as usize;

        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));
        let mut handles = Vec::new();

        for (name, prompt, model, allowed_tools, max_budget, cwd) in defs {
            let config = self.config.clone();
            let sem = semaphore.clone();
            let global_tx = self.global_event_tx.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let agent_config = AgentConfig {
                    model,
                    allowed_tools,
                    max_budget_usd: max_budget,
                    working_directory: cwd,
                    priority: AgentPriority::Normal,
                    ..Default::default()
                };
                let result = run_single_agent(&config, prompt, agent_config, Some(global_tx)).await;
                match result {
                    Ok(r) => (
                        name,
                        r.output,
                        r.total_cost_usd,
                        r.metrics.input_tokens,
                        r.metrics.output_tokens,
                        r.duration_ms,
                        true,
                    ),
                    Err(e) => (name, format!("Error: {}", e), 0.0, 0, 0, 0, false),
                }
            }));
        }

        let mut results = Vec::new();
        let mut total_cost = 0.0;
        let mut total_tokens = 0u64;

        for handle in handles {
            let (name, output, cost, input_tok, output_tok, duration, success) =
                handle.await.map_err(|e| e.to_string())?;
            total_cost += cost;
            total_tokens += input_tok + output_tok;
            let status = if success { "completed" } else { "failed" };
            results.push(format!(
                "## Agent: {} [{}]\n**Duration:** {}ms | **Cost:** ${:.4} | **Tokens:** {} in / {} out\n\n{}\n",
                name, status, duration, cost, input_tok, output_tok, output
            ));
        }

        Ok(vec![ToolContent::text(format!(
            "# RA Parallel Execution Complete\n**Agents:** {} | **Total cost:** ${:.4} | **Total tokens:** {}\n\n---\n\n{}",
            results.len(), total_cost, total_tokens, results.join("\n---\n\n")
        ))])
    }

    // =====================================================================
    // V2: Async ra_run_agents_async (fire-and-poll)
    // =====================================================================

    pub async fn run_agents_async(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let defs = Self::parse_agent_defs(&params)?;
        let max_concurrency = params
            .get("max_concurrency")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.runtime.max_concurrency as u64)
            as usize;

        let mut run_state = RunState::new();
        let run_id = run_state.run_id;

        // Initialize agent statuses
        for (name, _, _, _, _, _) in &defs {
            run_state.agents.insert(
                name.clone(),
                AgentRunStatus {
                    name: name.clone(),
                    agent_id: Uuid::new_v4(),
                    state: AgentState::Pending,
                    output: None,
                    error: None,
                    metrics: AgentMetrics::default(),
                    started_at: None,
                    completed_at: None,
                },
            );
        }

        self.active_runs.write().await.insert(run_id, run_state);

        // Spawn background execution
        let runs = self.active_runs.clone();
        let config = self.config.clone();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));
        let global_tx = self.global_event_tx.clone();

        tokio::spawn(async move {
            let mut join_handles = Vec::new();

            for (name, prompt, model, allowed_tools, max_budget, cwd) in defs {
                let config = config.clone();
                let sem = semaphore.clone();
                let runs = runs.clone();
                let global_tx = global_tx.clone();
                let rid = run_id;

                join_handles.push(tokio::spawn(async move {
                    // Mark as running
                    {
                        let mut r = runs.write().await;
                        if let Some(state) = r.get_mut(&rid) {
                            if let Some(agent) = state.agents.get_mut(&name) {
                                agent.state = AgentState::Running;
                                agent.started_at = Some(Utc::now());
                            }
                        }
                    }

                    let _permit = sem.acquire().await.unwrap();
                    let agent_config = AgentConfig {
                        model,
                        allowed_tools,
                        max_budget_usd: max_budget,
                        working_directory: cwd,
                        priority: AgentPriority::Normal,
                        ..Default::default()
                    };

                    let result =
                        run_single_agent(&config, prompt, agent_config, Some(global_tx.clone()))
                            .await;

                    // Update state
                    {
                        let mut r = runs.write().await;
                        if let Some(state) = r.get_mut(&rid) {
                            if let Some(agent) = state.agents.get_mut(&name) {
                                match result {
                                    Ok(res) => {
                                        agent.state = AgentState::Completed;
                                        agent.output = Some(res.output);
                                        agent.metrics = res.metrics;
                                        agent.completed_at = Some(Utc::now());
                                    }
                                    Err(e) => {
                                        agent.state = AgentState::Failed;
                                        agent.error = Some(e.to_string());
                                        agent.completed_at = Some(Utc::now());
                                    }
                                }
                            }
                            state.update_status();
                        }
                    }
                }));
            }

            // Wait for all to finish
            for h in join_handles {
                let _ = h.await;
            }

            // Final status update
            {
                let mut r = runs.write().await;
                if let Some(state) = r.get_mut(&run_id) {
                    state.update_status();
                }
            }
        });

        Ok(vec![ToolContent::text(format!(
            "# Async Run Launched\n**run_id:** `{}`\n\nUse `ra_get_run_status` to check progress.\nUse `ra_get_agent_output` to retrieve individual agent results.",
            run_id
        ))])
    }

    // =====================================================================
    // V2: ra_get_run_status
    // =====================================================================

    pub async fn get_run_status(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let run_id_str = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'run_id'")?;
        let run_id = Uuid::parse_str(run_id_str).map_err(|e| format!("Invalid run_id: {}", e))?;

        let runs = self.active_runs.read().await;
        let state = runs
            .get(&run_id)
            .ok_or(format!("Run {} not found", run_id))?;

        let mut output = format!(
            "# Run Status: {:?}\n**Run ID:** `{}`\n**Agents:** {} total | {} completed | {} running | {} failed\n**Cost:** ${:.4} | **Tokens:** {}\n\n",
            state.status,
            state.run_id,
            state.agents.len(),
            state.completed_count(),
            state.running_count(),
            state.failed_count(),
            state.total_cost_usd,
            state.total_tokens,
        );

        output.push_str("| Agent | State | Tokens | Cost |\n|---|---|---|---|\n");
        for (name, agent) in &state.agents {
            let tokens = agent.metrics.total_tokens();
            output.push_str(&format!(
                "| {} | {:?} | {} | ${:.4} |\n",
                name, agent.state, tokens, agent.metrics.total_cost_usd,
            ));
        }

        // If completed, show brief output snippets
        if state.status != RunStatus::Running {
            output.push_str("\n## Results\n\n");
            for (name, agent) in &state.agents {
                if let Some(ref out) = agent.output {
                    let snippet = if out.len() > 200 {
                        format!("{}...", &out[..200])
                    } else {
                        out.clone()
                    };
                    output.push_str(&format!("**{}:** {}\n\n", name, snippet));
                }
                if let Some(ref err) = agent.error {
                    output.push_str(&format!("**{} (error):** {}\n\n", name, err));
                }
            }
        }

        Ok(vec![ToolContent::text(output)])
    }

    // =====================================================================
    // V2: ra_get_agent_output
    // =====================================================================

    pub async fn get_agent_output(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let run_id_str = params
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'run_id'")?;
        let agent_name = params
            .get("agent_name")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'agent_name'")?;

        let run_id = Uuid::parse_str(run_id_str).map_err(|e| format!("Invalid run_id: {}", e))?;

        let runs = self.active_runs.read().await;
        let state = runs
            .get(&run_id)
            .ok_or(format!("Run {} not found", run_id))?;

        let agent = state.agents.get(agent_name).ok_or(format!(
            "Agent '{}' not found in run {}",
            agent_name, run_id
        ))?;

        match agent.state {
            AgentState::Completed => {
                let output = agent.output.as_deref().unwrap_or("(no output)");
                Ok(vec![ToolContent::text(format!(
                    "# Agent: {} [Completed]\n**Tokens:** {} in / {} out | **Cost:** ${:.4} | **Duration:** {}ms\n\n{}",
                    agent_name,
                    agent.metrics.input_tokens,
                    agent.metrics.output_tokens,
                    agent.metrics.total_cost_usd,
                    agent.metrics.duration_ms,
                    output,
                ))])
            }
            AgentState::Failed => {
                let error = agent.error.as_deref().unwrap_or("Unknown error");
                Ok(vec![ToolContent::text(format!(
                    "# Agent: {} [Failed]\n\n{}",
                    agent_name, error
                ))])
            }
            AgentState::Running => Ok(vec![ToolContent::text(format!(
                "# Agent: {} [Running]\n\nAgent is still running. Check back later.",
                agent_name
            ))]),
            other => Ok(vec![ToolContent::text(format!(
                "# Agent: {} [{:?}]",
                agent_name, other
            ))]),
        }
    }

    // =====================================================================
    // V2: Template methods
    // =====================================================================

    pub async fn list_templates(&self) -> Result<Vec<ToolContent>, String> {
        let templates = self.discover_templates();

        if templates.is_empty() {
            return Ok(vec![ToolContent::text("No templates found.")]);
        }

        let mut output = String::from("# Available Templates\n\n");
        for t in &templates {
            output.push_str(&format!("## {}\n{}\n\n", t.name, t.description));
            if !t.parameters.is_empty() {
                output.push_str("**Parameters:**\n");
                for p in &t.parameters {
                    let req = if p.required { " (required)" } else { "" };
                    let def = p
                        .default
                        .as_ref()
                        .map(|d| format!(" [default: {}]", d))
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "- `{}`: {}{}{}\n",
                        p.name, p.description, req, def
                    ));
                }
            }
            output.push('\n');
        }

        Ok(vec![ToolContent::text(output)])
    }

    pub async fn run_template(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let template_name = params
            .get("template")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'template' name")?;

        let user_params: HashMap<String, String> = params
            .get("parameters")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let is_async = params
            .get("async")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let template = self
            .load_template(template_name)
            .ok_or(format!("Template '{}' not found", template_name))?;

        let resolved_yaml = template
            .resolve(&user_params)
            .map_err(|e| format!("Template parameter error: {}", e))?;

        if is_async {
            // Run as workflow via the async path (future enhancement)
            self.run_workflow(serde_json::json!({ "workflow_yaml": resolved_yaml }))
                .await
        } else {
            self.run_workflow(serde_json::json!({ "workflow_yaml": resolved_yaml }))
                .await
        }
    }

    fn discover_templates(&self) -> Vec<TemplateInfo> {
        let mut templates = Vec::new();

        // Embedded templates
        for (name, yaml) in Self::embedded_templates() {
            if let Ok(info) = Self::parse_template_info(name, yaml) {
                templates.push(info);
            }
        }

        // Search filesystem: ./templates/ and ~/.ra/templates/
        for dir in &[
            "./templates",
            &RuntimeConfig::expand_path("~/.ra/templates"),
        ] {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .map(|e| e == "yaml" || e == "yml")
                        .unwrap_or(false)
                    {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown");
                            if let Ok(mut info) = Self::parse_template_info(name, &content) {
                                info.source =
                                    TemplateSource::File(path.to_string_lossy().to_string());
                                templates.push(info);
                            }
                        }
                    }
                }
            }
        }

        templates
    }

    fn load_template(&self, name: &str) -> Option<Template> {
        // Check embedded first
        for (tname, yaml) in Self::embedded_templates() {
            if tname == name {
                if let Ok(info) = Self::parse_template_info(tname, yaml) {
                    return Some(Template {
                        info,
                        yaml_content: yaml.to_string(),
                    });
                }
            }
        }

        // Check filesystem
        for dir in &[
            "./templates",
            &RuntimeConfig::expand_path("~/.ra/templates"),
        ] {
            for ext in &["yaml", "yml"] {
                let path = format!("{}/{}.{}", dir, name, ext);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(info) = Self::parse_template_info(name, &content) {
                        return Some(Template {
                            info,
                            yaml_content: content,
                        });
                    }
                }
            }
        }

        None
    }

    fn parse_template_info(name: &str, yaml: &str) -> Result<TemplateInfo, String> {
        let value: serde_json::Value = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;

        let desc = value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let parameters = value
            .get("parameters")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let pname = p.get("name")?.as_str()?.to_string();
                        let pdesc = p
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let required = p.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
                        let default = p
                            .get("default")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        Some(TemplateParameter {
                            name: pname,
                            description: pdesc,
                            required,
                            default,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(TemplateInfo {
            name: name.to_string(),
            description: desc,
            parameters,
            source: TemplateSource::Embedded,
        })
    }

    fn embedded_templates() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "pre-pr-review",
                include_str!("../../../templates/pre-pr-review.yaml"),
            ),
            (
                "codebase-onboard",
                include_str!("../../../templates/codebase-onboard.yaml"),
            ),
            ("bug-hunt", include_str!("../../../templates/bug-hunt.yaml")),
            (
                "migration-planner",
                include_str!("../../../templates/migration-planner.yaml"),
            ),
            (
                "test-generator",
                include_str!("../../../templates/test-generator.yaml"),
            ),
        ]
    }

    // =====================================================================
    // V1: Existing methods (unchanged)
    // =====================================================================

    pub async fn run_workflow(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let yaml_content = if let Some(yaml) = params.get("workflow_yaml").and_then(|v| v.as_str())
        {
            yaml.to_string()
        } else if let Some(path) = params.get("workflow_path").and_then(|v| v.as_str()) {
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| format!("Failed to read workflow file: {}", e))?
        } else {
            return Err("Provide either 'workflow_yaml' or 'workflow_path'".to_string());
        };

        // Use global event channel so IPC broadcaster gets events
        let event_tx = self.global_event_tx.clone();
        let checkpoint_store = self.db.as_ref().map(|db| {
            Arc::new(SqliteCheckpointStore::new(db.conn.clone()))
                as Arc<dyn ra_core::CheckpointStore>
        });

        let runner = WorkflowRunner::new(&self.config, event_tx, checkpoint_store);
        let result = runner
            .run_yaml(&yaml_content)
            .await
            .map_err(|e| format!("Workflow execution failed: {}", e))?;

        let mut output = format!(
            "# Workflow Complete\n**State:** {:?}\n**Agents:** {}/{} completed\n**Total cost:** ${:.4}\n**Total tokens:** {} in / {} out\n",
            result.state, result.metrics.completed_agents, result.metrics.total_agents,
            result.metrics.total_cost_usd, result.metrics.total_input_tokens, result.metrics.total_output_tokens,
        );

        if !result.step_outputs.is_empty() {
            output.push_str("\n---\n\n## Step Outputs\n\n");
            for (key, value) in &result.step_outputs {
                output.push_str(&format!("### {}\n{}\n\n", key, value));
            }
        }

        Ok(vec![ToolContent::text(output)])
    }

    pub async fn agent_status(&self) -> Result<Vec<ToolContent>, String> {
        let runs = self.active_runs.read().await;
        let active: Vec<_> = runs
            .values()
            .filter(|r| r.status == RunStatus::Running)
            .collect();

        if active.is_empty() {
            return Ok(vec![ToolContent::text(
                "No active runs. Use `ra_run_agents_async` to launch agents.",
            )]);
        }

        let mut output = String::from("# Active Runs\n\n");
        for run in active {
            output.push_str(&format!(
                "**Run {}:** {} agents ({} running, {} completed)\n",
                &run.run_id.to_string()[..8],
                run.agents.len(),
                run.running_count(),
                run.completed_count(),
            ));
        }

        Ok(vec![ToolContent::text(output)])
    }

    pub async fn metrics(&self) -> Result<Vec<ToolContent>, String> {
        let db = self.db.as_ref().ok_or("No database configured")?;
        let store = HistoryStore::new(db.conn.clone());
        let executions = store.list_executions(1).await.map_err(|e| e.to_string())?;

        if executions.is_empty() {
            return Ok(vec![ToolContent::text("No executions recorded yet.")]);
        }

        let exec = &executions[0];
        Ok(vec![ToolContent::text(format!(
            "# Last Execution Metrics\n**ID:** {}\n**Name:** {}\n**Status:** {}\n**Cost:** ${:.4}\n**Tokens:** {}\n**Started:** {}",
            exec.id, exec.name, exec.status, exec.total_cost_usd, exec.total_tokens, exec.started_at,
        ))])
    }

    pub async fn history(&self, params: serde_json::Value) -> Result<Vec<ToolContent>, String> {
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let db = self.db.as_ref().ok_or("No database configured")?;
        let store = HistoryStore::new(db.conn.clone());
        let executions = store
            .list_executions(limit)
            .await
            .map_err(|e| e.to_string())?;

        if executions.is_empty() {
            return Ok(vec![ToolContent::text("No execution history.")]);
        }

        let mut output = String::from("# Execution History\n\n| ID | Name | Status | Cost | Tokens |\n|---|---|---|---|---|\n");
        for exec in &executions {
            output.push_str(&format!(
                "| {} | {} | {} | ${:.4} | {} |\n",
                &exec.id.to_string()[..8],
                exec.name,
                exec.status,
                exec.total_cost_usd,
                exec.total_tokens,
            ));
        }

        Ok(vec![ToolContent::text(output)])
    }

    pub async fn checkpoint_list(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let wf_id_str = params
            .get("workflow_id")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'workflow_id'")?;
        let wf_id = Uuid::parse_str(wf_id_str).map_err(|e| format!("Invalid UUID: {}", e))?;
        let db = self.db.as_ref().ok_or("No database configured")?;
        let store = SqliteCheckpointStore::new(db.conn.clone());
        let checkpoints = ra_core::CheckpointStore::list(&store, wf_id)
            .await
            .map_err(|e| e.to_string())?;

        if checkpoints.is_empty() {
            return Ok(vec![ToolContent::text("No checkpoints found.")]);
        }

        let mut output = String::from("# Checkpoints\n\n| ID | Created | State |\n|---|---|---|\n");
        for cp in &checkpoints {
            output.push_str(&format!(
                "| {} | {} | {:?} |\n",
                &cp.id.to_string()[..8],
                cp.created_at.format("%Y-%m-%d %H:%M:%S"),
                cp.workflow_state,
            ));
        }

        Ok(vec![ToolContent::text(output)])
    }

    // =====================================================================
    // Workflow Builder tools
    // =====================================================================

    /// Validate a workflow YAML without executing it
    pub async fn validate_workflow(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let yaml = params
            .get("workflow_yaml")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'workflow_yaml'")?;

        // Parse YAML
        let workflow: ra_core::Workflow =
            serde_yaml::from_str(yaml).map_err(|e| format!("YAML parse error: {}", e))?;

        // Validate DAG
        match workflow.validate() {
            Ok(()) => {
                let step_count = workflow.steps.len();
                let parallel_roots = workflow
                    .steps
                    .iter()
                    .filter(|s| s.depends_on.is_empty())
                    .count();
                let has_output_vars = workflow
                    .steps
                    .iter()
                    .filter(|s| s.output_var.is_some())
                    .count();

                let mut output = format!(
                    "# Workflow Valid\n\n**Name:** {}\n**Steps:** {}\n**Parallel roots:** {} (steps with no dependencies)\n**Output variables:** {}\n\n",
                    workflow.name, step_count, parallel_roots, has_output_vars,
                );

                // Show execution order
                if let Ok(order) = workflow.topological_order() {
                    output.push_str("**Execution order:**\n");
                    for (i, step) in order.iter().enumerate() {
                        let deps = if step.depends_on.is_empty() {
                            "ready immediately".to_string()
                        } else {
                            format!(
                                "after: {}",
                                step.depends_on
                                    .iter()
                                    .map(|d| d.step_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };
                        output.push_str(&format!("{}. `{}` — {}\n", i + 1, step.id, deps));
                    }
                }

                // Check for potential issues (warnings, not errors)
                let mut warnings = Vec::new();

                for step in &workflow.steps {
                    if step.prompt.contains("{{") {
                        // Check if referenced variables have producers
                        let all_output_vars: Vec<&str> = workflow
                            .steps
                            .iter()
                            .filter_map(|s| s.output_var.as_deref())
                            .collect();
                        for part in step.prompt.split("{{") {
                            if let Some(end) = part.find("}}") {
                                let var_name = &part[..end];
                                if !all_output_vars.contains(&var_name) {
                                    warnings.push(format!(
                                        "Step '{}' references `{{{{{}}}}}` but no step produces this variable",
                                        step.id, var_name
                                    ));
                                }
                            }
                        }
                    }
                }

                if !warnings.is_empty() {
                    output.push_str("\n## Warnings\n\n");
                    for w in &warnings {
                        output.push_str(&format!("- {}\n", w));
                    }
                }

                Ok(vec![ToolContent::text(output)])
            }
            Err(e) => Ok(vec![ToolContent::text(format!(
                "# Workflow Invalid\n\n**Error:** {}\n\nFix the error and validate again.",
                e
            ))]),
        }
    }

    /// Save a workflow YAML as a reusable template
    pub async fn save_workflow(
        &self,
        params: serde_json::Value,
    ) -> Result<Vec<ToolContent>, String> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'name'")?;
        let yaml = params
            .get("workflow_yaml")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'workflow_yaml'")?;
        let description = params.get("description").and_then(|v| v.as_str());

        // Validate name (lowercase, hyphens, alphanumeric)
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err("Name must be lowercase letters, digits, and hyphens only".to_string());
        }

        // Validate the YAML first
        let _workflow: ra_core::Workflow =
            serde_yaml::from_str(yaml).map_err(|e| format!("YAML parse error: {}", e))?;
        _workflow
            .validate()
            .map_err(|e| format!("Workflow validation error: {}", e))?;

        // Ensure description is in the YAML
        let final_yaml = if let Some(desc) = description {
            if !yaml.contains("description:") {
                yaml.replacen(
                    &format!("name: \"{}\"", _workflow.name),
                    &format!("name: \"{}\"\ndescription: \"{}\"", _workflow.name, desc),
                    1,
                )
            } else {
                yaml.to_string()
            }
        } else {
            yaml.to_string()
        };

        // Save to ~/.ra/templates/
        let templates_dir = ra_core::config::RuntimeConfig::expand_path("~/.ra/templates");
        std::fs::create_dir_all(&templates_dir)
            .map_err(|e| format!("Failed to create templates directory: {}", e))?;

        let file_path = format!("{}/{}.yaml", templates_dir, name);
        std::fs::write(&file_path, &final_yaml)
            .map_err(|e| format!("Failed to write template file: {}", e))?;

        Ok(vec![ToolContent::text(format!(
            "# Template Saved\n\n**Name:** `{}`\n**Path:** `{}`\n\nYou can now run it with:\n- `ra_run_template` with `template: \"{}\"`\n- CLI: `ra run {}`",
            name, file_path, name, file_path,
        ))])
    }
}
