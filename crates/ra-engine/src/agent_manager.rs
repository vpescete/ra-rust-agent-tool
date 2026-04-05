use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use ra_core::agent::{Agent, AgentState};
use ra_core::error::{RaError, RaResult};
use ra_core::event::AgentEvent;

use crate::process::ClaudeProcess;

struct AgentEntry {
    agent: Agent,
    cancel: CancellationToken,
    _join_handle: JoinHandle<RaResult<i32>>,
}

pub struct AgentManager {
    claude_binary: String,
    agents: Arc<RwLock<HashMap<Uuid, AgentEntry>>>,
    event_broadcast: broadcast::Sender<AgentEvent>,
}

impl AgentManager {
    pub fn new(claude_binary: String, event_broadcast: broadcast::Sender<AgentEvent>) -> Self {
        Self {
            claude_binary,
            agents: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast,
        }
    }

    /// Spawn an agent and return a receiver for its events.
    /// Events are ALSO forwarded to the broadcast channel (for IPC/observability).
    pub async fn spawn_agent(
        &self,
        mut agent: Agent,
    ) -> RaResult<mpsc::UnboundedReceiver<AgentEvent>> {
        let (process_tx, mut process_rx) = mpsc::unbounded_channel();
        let (caller_tx, caller_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();

        agent
            .transition_to(AgentState::Running)
            .map_err(RaError::WorkflowValidation)?;

        // Emit state change to broadcast
        let state_event = AgentEvent::StateChanged {
            agent_id: agent.id,
            old: AgentState::Pending,
            new: AgentState::Running,
        };
        self.event_broadcast.send(state_event).ok();

        // Spawn the claude process — writes events to process_tx
        let process =
            ClaudeProcess::spawn(&agent, &self.claude_binary, process_tx, cancel.clone())?;

        let agent_id = agent.id;
        let broadcast_tx = self.event_broadcast.clone();
        let agents_ref = self.agents.clone();

        // Forwarder task: reads from process_rx, sends to BOTH caller_tx and broadcast_tx
        let fwd_broadcast = self.event_broadcast.clone();
        tokio::spawn(async move {
            while let Some(event) = process_rx.recv().await {
                // Forward to broadcast (for IPC socket / dashboard)
                fwd_broadcast.send(event.clone()).ok();
                // Forward to caller (for the DAG engine / handler)
                caller_tx.send(event).ok();
            }
        });

        // Process runner task
        let join = tokio::spawn(async move {
            let result = process.run().await;

            match &result {
                Ok(code) => {
                    let new_state = if *code == 0 {
                        AgentState::Completed
                    } else if *code == -1 {
                        AgentState::Killed
                    } else {
                        AgentState::Failed
                    };
                    broadcast_tx
                        .send(AgentEvent::StateChanged {
                            agent_id,
                            old: AgentState::Running,
                            new: new_state,
                        })
                        .ok();
                }
                Err(e) => {
                    broadcast_tx
                        .send(AgentEvent::Error {
                            agent_id,
                            error: e.to_string(),
                        })
                        .ok();
                }
            }

            agents_ref.write().await.remove(&agent_id);
            result
        });

        let entry = AgentEntry {
            agent,
            cancel,
            _join_handle: join,
        };
        self.agents.write().await.insert(agent_id, entry);

        Ok(caller_rx)
    }

    /// Kill a running agent
    pub async fn kill_agent(&self, agent_id: Uuid) -> RaResult<()> {
        let agents = self.agents.read().await;
        if let Some(entry) = agents.get(&agent_id) {
            entry.cancel.cancel();
            Ok(())
        } else {
            Err(RaError::StepNotFound(agent_id.to_string()))
        }
    }

    /// List all active agents
    pub async fn list_agents(&self) -> Vec<Agent> {
        let agents = self.agents.read().await;
        agents.values().map(|e| e.agent.clone()).collect()
    }

    /// Get the number of active agents
    pub async fn active_count(&self) -> usize {
        self.agents.read().await.len()
    }

    /// Kill all running agents
    pub async fn kill_all(&self) {
        let agents = self.agents.read().await;
        for entry in agents.values() {
            entry.cancel.cancel();
        }
    }
}
