use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ra_core::error::{RaError, RaResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Uuid,
    pub name: String,
    pub workflow_name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub total_cost_usd: f64,
    pub total_tokens: i64,
}

pub struct HistoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl HistoryStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    pub async fn save_execution(&self, exec: &Execution) -> RaResult<()> {
        let id = exec.id.to_string();
        let name = exec.name.clone();
        let wf_name = exec.workflow_name.clone();
        let started = exec.started_at.to_rfc3339();
        let completed = exec.completed_at.map(|t| t.to_rfc3339());
        let status = exec.status.clone();
        let cost = exec.total_cost_usd;
        let tokens = exec.total_tokens;
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO executions (id, name, workflow_name, started_at, completed_at, status, total_cost_usd, total_tokens) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, name, wf_name, started, completed, status, cost, tokens],
            )
            .map_err(|e| RaError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }

    pub async fn update_execution(
        &self,
        id: Uuid,
        status: &str,
        cost: f64,
        tokens: i64,
    ) -> RaResult<()> {
        let id_str = id.to_string();
        let status = status.to_string();
        let completed = Utc::now().to_rfc3339();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE executions SET status = ?1, completed_at = ?2, total_cost_usd = ?3, total_tokens = ?4 WHERE id = ?5",
                params![status, completed, cost, tokens, id_str],
            )
            .map_err(|e| RaError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }

    pub async fn list_executions(&self, limit: usize) -> RaResult<Vec<Execution>> {
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, workflow_name, started_at, completed_at, status, total_cost_usd, total_tokens FROM executions ORDER BY started_at DESC LIMIT ?1",
                )
                .map_err(|e| RaError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let id_str: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let wf_name: Option<String> = row.get(2)?;
                    let started_str: String = row.get(3)?;
                    let completed_str: Option<String> = row.get(4)?;
                    let status: String = row.get(5)?;
                    let cost: f64 = row.get(6)?;
                    let tokens: i64 = row.get(7)?;

                    Ok((id_str, name, wf_name, started_str, completed_str, status, cost, tokens))
                })
                .map_err(|e| RaError::Database(e.to_string()))?;

            let mut executions = Vec::new();
            for row in rows {
                let (id_str, name, wf_name, started_str, completed_str, status, cost, tokens) =
                    row.map_err(|e| RaError::Database(e.to_string()))?;

                let id = Uuid::parse_str(&id_str)
                    .map_err(|e| RaError::Database(e.to_string()))?;
                let started_at = DateTime::parse_from_rfc3339(&started_str)
                    .map_err(|e| RaError::Database(e.to_string()))?
                    .with_timezone(&Utc);
                let completed_at = completed_str
                    .map(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|t| t.with_timezone(&Utc))
                            .ok()
                    })
                    .flatten();

                executions.push(Execution {
                    id,
                    name,
                    workflow_name: wf_name,
                    started_at,
                    completed_at,
                    status,
                    total_cost_usd: cost,
                    total_tokens: tokens,
                });
            }
            Ok(executions)
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[tokio::test]
    async fn test_save_and_list() {
        let db = Database::open_in_memory().unwrap();
        let store = HistoryStore::new(db.conn.clone());

        let exec = Execution {
            id: Uuid::new_v4(),
            name: "test-run".to_string(),
            workflow_name: Some("my-workflow".to_string()),
            started_at: Utc::now(),
            completed_at: None,
            status: "running".to_string(),
            total_cost_usd: 0.0,
            total_tokens: 0,
        };

        store.save_execution(&exec).await.unwrap();

        let list = store.list_executions(10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-run");
        assert_eq!(list[0].status, "running");
    }

    #[tokio::test]
    async fn test_update_execution() {
        let db = Database::open_in_memory().unwrap();
        let store = HistoryStore::new(db.conn.clone());

        let exec_id = Uuid::new_v4();
        let exec = Execution {
            id: exec_id,
            name: "test-run".to_string(),
            workflow_name: None,
            started_at: Utc::now(),
            completed_at: None,
            status: "running".to_string(),
            total_cost_usd: 0.0,
            total_tokens: 0,
        };

        store.save_execution(&exec).await.unwrap();
        store
            .update_execution(exec_id, "completed", 0.05, 1500)
            .await
            .unwrap();

        let list = store.list_executions(10).await.unwrap();
        assert_eq!(list[0].status, "completed");
        assert!(list[0].completed_at.is_some());
        assert!((list[0].total_cost_usd - 0.05).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_list_limit() {
        let db = Database::open_in_memory().unwrap();
        let store = HistoryStore::new(db.conn.clone());

        for i in 0..5 {
            let exec = Execution {
                id: Uuid::new_v4(),
                name: format!("run-{}", i),
                workflow_name: None,
                started_at: Utc::now(),
                completed_at: None,
                status: "completed".to_string(),
                total_cost_usd: 0.0,
                total_tokens: 0,
            };
            store.save_execution(&exec).await.unwrap();
        }

        let list = store.list_executions(3).await.unwrap();
        assert_eq!(list.len(), 3);
    }
}
