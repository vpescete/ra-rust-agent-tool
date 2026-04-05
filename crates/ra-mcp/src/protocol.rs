//! JSON-RPC 2.0 types for MCP protocol

use serde::{Deserialize, Serialize};

/// JSON-RPC request
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC success response
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: serde_json::Value,
}

/// JSON-RPC error response
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub error: RpcErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcErrorBody {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn new(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result,
        }
    }
}

impl JsonRpcError {
    pub fn new(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            error: RpcErrorBody {
                code,
                message: message.into(),
            },
        }
    }

    pub fn method_not_found(id: serde_json::Value, method: &str) -> Self {
        Self::new(id, -32601, format!("Method not found: {}", method))
    }

    #[allow(dead_code)]
    pub fn invalid_params(id: serde_json::Value, msg: impl Into<String>) -> Self {
        Self::new(id, -32602, msg)
    }

    #[allow(dead_code)]
    pub fn internal_error(id: serde_json::Value, msg: impl Into<String>) -> Self {
        Self::new(id, -32603, msg)
    }
}

/// MCP Tool definition
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP tool call result content
#[derive(Debug, Clone, Serialize)]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl ToolContent {
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            content_type: "text".to_string(),
            text: s.into(),
        }
    }
}
