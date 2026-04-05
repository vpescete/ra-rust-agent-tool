use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{params, Connection};
use uuid::Uuid;

use ra_core::checkpoint::Checkpoint;
use ra_core::error::{RaError, RaResult};
use ra_core::CheckpointStore;

pub struct SqliteCheckpointStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteCheckpointStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CheckpointStore for SqliteCheckpointStore {
    async fn save(&self, checkpoint: &Checkpoint) -> RaResult<Uuid> {
        let json =
            serde_json::to_string(checkpoint).map_err(|e| RaError::Database(e.to_string()))?;
        let cp_id = checkpoint.id;
        let id = cp_id.to_string();
        let wf_id = checkpoint.workflow_id.to_string();
        let created = checkpoint.created_at.to_rfc3339();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO checkpoints (id, workflow_id, created_at, payload) VALUES (?1, ?2, ?3, ?4)",
                params![id, wf_id, created, json],
            )
            .map_err(|e| RaError::Database(e.to_string()))?;
            Ok(cp_id)
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }

    async fn load(&self, id: Uuid) -> RaResult<Option<Checkpoint>> {
        let id_str = id.to_string();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let result = conn.query_row(
                "SELECT payload FROM checkpoints WHERE id = ?1",
                params![id_str],
                |row| {
                    let json: String = row.get(0)?;
                    Ok(json)
                },
            );

            match result {
                Ok(json) => {
                    let checkpoint: Checkpoint = serde_json::from_str(&json)
                        .map_err(|e| RaError::Database(e.to_string()))?;
                    Ok(Some(checkpoint))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(RaError::Database(e.to_string())),
            }
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }

    async fn list(&self, workflow_id: Uuid) -> RaResult<Vec<Checkpoint>> {
        let wf_id = workflow_id.to_string();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT payload FROM checkpoints WHERE workflow_id = ?1 ORDER BY created_at DESC")
                .map_err(|e| RaError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![wf_id], |row| {
                    let json: String = row.get(0)?;
                    Ok(json)
                })
                .map_err(|e| RaError::Database(e.to_string()))?;

            let mut checkpoints = Vec::new();
            for row in rows {
                let json = row.map_err(|e| RaError::Database(e.to_string()))?;
                let cp: Checkpoint =
                    serde_json::from_str(&json).map_err(|e| RaError::Database(e.to_string()))?;
                checkpoints.push(cp);
            }
            Ok(checkpoints)
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }

    async fn latest(&self, workflow_id: Uuid) -> RaResult<Option<Checkpoint>> {
        let wf_id = workflow_id.to_string();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let result = conn.query_row(
                "SELECT payload FROM checkpoints WHERE workflow_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![wf_id],
                |row| {
                    let json: String = row.get(0)?;
                    Ok(json)
                },
            );

            match result {
                Ok(json) => {
                    let checkpoint: Checkpoint =
                        serde_json::from_str(&json).map_err(|e| RaError::Database(e.to_string()))?;
                    Ok(Some(checkpoint))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(RaError::Database(e.to_string())),
            }
        })
        .await
        .map_err(|e| RaError::Database(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use ra_core::checkpoint::{Checkpoint, WorkflowState};

    #[tokio::test]
    async fn test_save_and_load() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteCheckpointStore::new(db.conn.clone());

        let wf_id = Uuid::new_v4();
        let cp = Checkpoint::new(wf_id);
        let cp_id = cp.id;

        // Ensure execution exists (foreign key)
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO executions (id, name, started_at, status) VALUES (?1, 'test', datetime('now'), 'running')",
                params![wf_id.to_string()],
            ).unwrap();
        }

        store.save(&cp).await.unwrap();

        let loaded = store.load(cp_id).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, cp_id);
        assert_eq!(loaded.workflow_id, wf_id);
        assert_eq!(loaded.workflow_state, WorkflowState::Running);
    }

    #[tokio::test]
    async fn test_load_nonexistent() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteCheckpointStore::new(db.conn.clone());

        let result = store.load(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_checkpoints() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteCheckpointStore::new(db.conn.clone());

        let wf_id = Uuid::new_v4();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO executions (id, name, started_at, status) VALUES (?1, 'test', datetime('now'), 'running')",
                params![wf_id.to_string()],
            ).unwrap();
        }

        let cp1 = Checkpoint::new(wf_id);
        let cp2 = Checkpoint::new(wf_id);
        store.save(&cp1).await.unwrap();
        store.save(&cp2).await.unwrap();

        let list = store.list(wf_id).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_latest() {
        let db = Database::open_in_memory().unwrap();
        let store = SqliteCheckpointStore::new(db.conn.clone());

        let wf_id = Uuid::new_v4();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO executions (id, name, started_at, status) VALUES (?1, 'test', datetime('now'), 'running')",
                params![wf_id.to_string()],
            ).unwrap();
        }

        let cp1 = Checkpoint::new(wf_id);
        let cp2 = Checkpoint::new(wf_id);
        let cp2_id = cp2.id;
        store.save(&cp1).await.unwrap();
        store.save(&cp2).await.unwrap();

        let latest = store.latest(wf_id).await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, cp2_id);
    }
}
