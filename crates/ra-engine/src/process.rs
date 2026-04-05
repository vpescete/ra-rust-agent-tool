use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use ra_core::agent::Agent;
use ra_core::error::{RaError, RaResult};
use ra_core::event::AgentEvent;
use ra_core::metrics::AgentMetrics;

use crate::stream_parser::{is_rate_limited, parse_stream_line, update_metrics};

/// Wrapper around a `claude` CLI subprocess
pub struct ClaudeProcess {
    child: tokio::process::Child,
    agent_id: Uuid,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: CancellationToken,
}

impl ClaudeProcess {
    /// Spawn a new claude process for the given agent
    pub fn spawn(
        agent: &Agent,
        claude_binary: &str,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        cancel: CancellationToken,
    ) -> RaResult<Self> {
        let mut cmd = tokio::process::Command::new(claude_binary);

        // Core flags
        cmd.arg("-p").arg(&agent.prompt);
        cmd.arg("--output-format").arg("stream-json");
        cmd.arg("--verbose");

        // Model
        if let Some(ref model) = agent.config.model {
            cmd.arg("--model").arg(model);
        }

        // Allowed tools
        if !agent.config.allowed_tools.is_empty() {
            cmd.arg("--allowedTools");
            for tool in &agent.config.allowed_tools {
                cmd.arg(tool);
            }
        }

        // Budget
        if let Some(budget) = agent.config.max_budget_usd {
            cmd.arg("--max-budget-usd").arg(budget.to_string());
        }

        // Session
        if let Some(ref sid) = agent.config.session_id {
            cmd.arg("--session-id").arg(sid.to_string());
        } else {
            cmd.arg("--no-session-persistence");
        }

        // System prompt
        if let Some(ref sys) = agent.config.system_prompt {
            cmd.arg("--system-prompt").arg(sys);
        }

        // MCP config
        if let Some(ref mcp) = agent.config.mcp_config {
            cmd.arg("--mcp-config").arg(mcp);
        }

        // Working directory
        if let Some(ref cwd) = agent.config.working_directory {
            cmd.current_dir(cwd);
        }

        // Extra args
        for arg in &agent.config.extra_args {
            cmd.arg(arg);
        }

        // Pipe I/O
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|e| RaError::ProcessSpawn(e.to_string()))?;

        Ok(Self {
            child,
            agent_id: agent.id,
            event_tx,
            cancel,
        })
    }

    /// Run the stream reading loop. Consumes self. Returns exit code.
    pub async fn run(mut self) -> RaResult<i32> {
        let stdout = self
            .child
            .stdout
            .take()
            .ok_or_else(|| RaError::ProcessSpawn("Failed to capture stdout".to_string()))?;

        let mut lines = BufReader::new(stdout).lines();
        let mut metrics = AgentMetrics::default();

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    self.child.kill().await.ok();
                    return Ok(-1);
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            if let Some(event) = parse_stream_line(&text) {
                                update_metrics(&mut metrics, &event);

                                if let Some(delay) = is_rate_limited(&event) {
                                    self.event_tx.send(AgentEvent::RateLimited {
                                        agent_id: self.agent_id,
                                        retry_after_ms: delay,
                                    }).ok();
                                }

                                self.event_tx.send(AgentEvent::StreamLine {
                                    agent_id: self.agent_id,
                                    event,
                                }).ok();

                                self.event_tx.send(AgentEvent::MetricsUpdated {
                                    agent_id: self.agent_id,
                                    metrics: metrics.clone(),
                                }).ok();
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.event_tx.send(AgentEvent::Error {
                                agent_id: self.agent_id,
                                error: e.to_string(),
                            }).ok();
                            break;
                        }
                    }
                }
            }
        }

        let status = self
            .child
            .wait()
            .await
            .map_err(|e| RaError::ProcessWait(e.to_string()))?;
        let code = status.code().unwrap_or(-1);

        self.event_tx
            .send(AgentEvent::ProcessExited {
                agent_id: self.agent_id,
                exit_code: code,
            })
            .ok();

        Ok(code)
    }
}
