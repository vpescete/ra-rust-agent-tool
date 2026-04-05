//! MCP tool definitions for RA

use crate::protocol::ToolDefinition;

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "ra_run_agents".to_string(),
            description: "Run multiple Claude agents in parallel with different prompts. Each agent runs independently and results are collected. Use this when you need to analyze something from multiple perspectives simultaneously, or when you need to perform independent tasks in parallel.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "description": "List of agents to run in parallel",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {
                                    "type": "string",
                                    "description": "Short name for this agent (e.g., 'security-review')"
                                },
                                "prompt": {
                                    "type": "string",
                                    "description": "The prompt to send to this agent"
                                },
                                "model": {
                                    "type": "string",
                                    "description": "Model to use (sonnet, opus, haiku). Default: sonnet"
                                },
                                "allowed_tools": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Tools this agent can use (e.g., ['Read', 'Glob', 'Grep'])"
                                },
                                "max_budget_usd": {
                                    "type": "number",
                                    "description": "Maximum cost in USD for this agent"
                                },
                                "working_directory": {
                                    "type": "string",
                                    "description": "Working directory for this agent. Default: current directory"
                                }
                            },
                            "required": ["name", "prompt"]
                        }
                    },
                    "max_concurrency": {
                        "type": "integer",
                        "description": "Maximum number of agents running simultaneously. Default: 4"
                    }
                },
                "required": ["agents"]
            }),
        },
        ToolDefinition {
            name: "ra_run_workflow".to_string(),
            description: "Execute a workflow defined in YAML with DAG dependencies between steps. Steps without dependencies run in parallel. Use this for complex multi-step pipelines where some steps depend on outputs of previous steps.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_yaml": {
                        "type": "string",
                        "description": "Complete workflow definition in YAML format. Must include 'name', 'steps' with 'id', 'prompt', and optionally 'depends_on', 'output_var', 'agent_config'."
                    },
                    "workflow_path": {
                        "type": "string",
                        "description": "Path to a workflow YAML file. Use this OR workflow_yaml, not both."
                    }
                }
            }),
        },
        ToolDefinition {
            name: "ra_agent_status".to_string(),
            description: "Get the current status of running agents, including active count and per-agent metrics.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ra_metrics".to_string(),
            description: "Get detailed metrics for the last execution: token usage, costs, duration per agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ra_history".to_string(),
            description: "List past workflow executions with their status, cost, and token usage.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Number of entries to show. Default: 10"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "ra_checkpoint_list".to_string(),
            description: "List available checkpoints for a workflow that can be used for resuming.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": {
                        "type": "string",
                        "description": "UUID of the workflow to list checkpoints for"
                    }
                },
                "required": ["workflow_id"]
            }),
        },
        // ===================== V2 Tools =====================
        ToolDefinition {
            name: "ra_run_agents_async".to_string(),
            description: "Launch multiple agents in parallel and return immediately with a run_id. Use ra_get_run_status to poll for progress and ra_get_agent_output to retrieve individual results. Prefer this over ra_run_agents for long-running tasks.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "description": "List of agents to run in parallel",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Short name for this agent" },
                                "prompt": { "type": "string", "description": "The prompt to send" },
                                "model": { "type": "string", "description": "Model to use (sonnet, opus, haiku)" },
                                "allowed_tools": { "type": "array", "items": { "type": "string" } },
                                "max_budget_usd": { "type": "number" },
                                "working_directory": { "type": "string" }
                            },
                            "required": ["name", "prompt"]
                        }
                    },
                    "max_concurrency": { "type": "integer", "description": "Max parallel agents. Default: 4" }
                },
                "required": ["agents"]
            }),
        },
        ToolDefinition {
            name: "ra_get_run_status".to_string(),
            description: "Get the current status of an async run. Shows which agents completed, which are running, partial results, and aggregate metrics.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "Run ID returned by ra_run_agents_async" }
                },
                "required": ["run_id"]
            }),
        },
        ToolDefinition {
            name: "ra_get_agent_output".to_string(),
            description: "Get the full output of a specific agent from an async run.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string", "description": "Run ID" },
                    "agent_name": { "type": "string", "description": "Name of the agent to get output for" }
                },
                "required": ["run_id", "agent_name"]
            }),
        },
        ToolDefinition {
            name: "ra_list_templates".to_string(),
            description: "List available workflow templates with descriptions and parameters. Templates are pre-built workflows for common tasks like code review, bug hunting, onboarding, etc.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ra_run_template".to_string(),
            description: "Run a workflow template by name with parameter substitution. Use ra_list_templates to see available templates.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "template": { "type": "string", "description": "Template name (e.g., 'pre-pr-review', 'bug-hunt', 'codebase-onboard')" },
                    "parameters": {
                        "type": "object",
                        "description": "Template parameters as key-value pairs",
                        "additionalProperties": { "type": "string" }
                    },
                    "async": { "type": "boolean", "description": "If true, returns run_id immediately. Default: false" }
                },
                "required": ["template"]
            }),
        },
        // ===================== Workflow Builder Tools =====================
        ToolDefinition {
            name: "ra_validate_workflow".to_string(),
            description: "Validate a workflow YAML without executing it. Checks for: DAG cycles, missing dependencies, duplicate step IDs, invalid references, and schema errors. Use this BEFORE running a workflow to catch errors early. Returns 'Valid' or a list of specific errors to fix.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_yaml": {
                        "type": "string",
                        "description": "Complete workflow YAML content to validate"
                    }
                },
                "required": ["workflow_yaml"]
            }),
        },
        ToolDefinition {
            name: "ra_save_workflow".to_string(),
            description: "Save a workflow YAML as a reusable template in ~/.ra/templates/. Once saved, it becomes available via ra_list_templates and ra_run_template. Use this after generating and validating a workflow YAML to make it reusable.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Template name (lowercase, hyphens, e.g., 'db-migration', 'api-review'). Used as filename and for ra_run_template."
                    },
                    "workflow_yaml": {
                        "type": "string",
                        "description": "Complete workflow YAML content to save"
                    },
                    "description": {
                        "type": "string",
                        "description": "One-line description of what this workflow does"
                    }
                },
                "required": ["name", "workflow_yaml"]
            }),
        },
    ]
}
