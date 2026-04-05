//! MCP server: JSON-RPC over stdio

use std::io::{self, BufRead, Write};

use crate::handler::Handler;
use crate::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, ToolContent};
use crate::tools;

pub struct McpServer {
    handler: Handler,
}

impl McpServer {
    pub fn new(handler: Handler) -> Self {
        Self { handler }
    }

    /// Run the MCP server loop: read JSON-RPC from stdin, write responses to stdout
    pub async fn run(&self) -> anyhow::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        let reader = stdin.lock();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break, // EOF or error
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON-RPC request
            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    let err = JsonRpcError::new(
                        serde_json::Value::Null,
                        -32700,
                        format!("Parse error: {}", e),
                    );
                    let response = serde_json::to_string(&err).unwrap();
                    writeln!(stdout, "{}", response)?;
                    stdout.flush()?;
                    continue;
                }
            };

            let id = request.id.clone().unwrap_or(serde_json::Value::Null);

            // Handle request
            let response = match request.method.as_str() {
                "initialize" => self.handle_initialize(id),
                "initialized" => {
                    // Notification, no response needed
                    continue;
                }
                "notifications/initialized" => {
                    continue;
                }
                "tools/list" => self.handle_tools_list(id),
                "tools/call" => self.handle_tools_call(id, request.params).await,
                "ping" => {
                    serde_json::to_string(&JsonRpcResponse::new(id, serde_json::json!({}))).unwrap()
                }
                _ => {
                    // Check if it's a notification (no id = notification)
                    if request.id.is_none() {
                        continue;
                    }
                    serde_json::to_string(&JsonRpcError::method_not_found(id, &request.method))
                        .unwrap()
                }
            };

            writeln!(stdout, "{}", response)?;
            stdout.flush()?;
        }

        Ok(())
    }

    fn handle_initialize(&self, id: serde_json::Value) -> String {
        let result = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "ra-mcp-server",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        serde_json::to_string(&JsonRpcResponse::new(id, result)).unwrap()
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> String {
        let tool_defs = tools::all_tools();
        let result = serde_json::json!({
            "tools": tool_defs
        });

        serde_json::to_string(&JsonRpcResponse::new(id, result)).unwrap()
    }

    async fn handle_tools_call(&self, id: serde_json::Value, params: serde_json::Value) -> String {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let result = match tool_name {
            "ra_run_agents" => self.handler.run_agents(arguments).await,
            "ra_run_workflow" => self.handler.run_workflow(arguments).await,
            "ra_agent_status" => self.handler.agent_status().await,
            "ra_metrics" => self.handler.metrics().await,
            "ra_history" => self.handler.history(arguments).await,
            "ra_checkpoint_list" => self.handler.checkpoint_list(arguments).await,
            // V2 tools
            "ra_run_agents_async" => self.handler.run_agents_async(arguments).await,
            "ra_get_run_status" => self.handler.get_run_status(arguments).await,
            "ra_get_agent_output" => self.handler.get_agent_output(arguments).await,
            "ra_list_templates" => self.handler.list_templates().await,
            "ra_run_template" => self.handler.run_template(arguments).await,
            // Workflow builder tools
            "ra_validate_workflow" => self.handler.validate_workflow(arguments).await,
            "ra_save_workflow" => self.handler.save_workflow(arguments).await,
            _ => Err(format!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => {
                let result = serde_json::json!({
                    "content": content,
                    "isError": false
                });
                serde_json::to_string(&JsonRpcResponse::new(id, result)).unwrap()
            }
            Err(e) => {
                let result = serde_json::json!({
                    "content": [ToolContent::text(format!("Error: {}", e))],
                    "isError": true
                });
                serde_json::to_string(&JsonRpcResponse::new(id, result)).unwrap()
            }
        }
    }
}
