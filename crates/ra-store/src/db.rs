use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use ra_core::error::{RaError, RaResult};

/// SQLite database wrapper
pub struct Database {
    pub conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Open or create a database at the given path
    pub fn open(path: &str) -> RaResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RaError::Database(format!("Failed to create db directory: {}", e)))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| RaError::Database(format!("Failed to open database: {}", e)))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing)
    pub fn open_in_memory() -> RaResult<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| RaError::Database(format!("Failed to open in-memory db: {}", e)))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> RaResult<()> {
        let conn = self.conn.lock().unwrap();

        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| RaError::Database(e.to_string()))?;

        if version < 1 {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS executions (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    workflow_name TEXT,
                    started_at TEXT NOT NULL,
                    completed_at TEXT,
                    status TEXT NOT NULL,
                    total_cost_usd REAL DEFAULT 0,
                    total_tokens INTEGER DEFAULT 0,
                    config TEXT
                );

                CREATE TABLE IF NOT EXISTS checkpoints (
                    id TEXT PRIMARY KEY,
                    workflow_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    payload TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS agent_logs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    execution_id TEXT NOT NULL,
                    agent_id TEXT NOT NULL,
                    step_id TEXT,
                    timestamp TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    payload TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_checkpoints_workflow ON checkpoints(workflow_id);
                CREATE INDEX IF NOT EXISTS idx_agent_logs_execution ON agent_logs(execution_id);
                CREATE INDEX IF NOT EXISTS idx_agent_logs_agent ON agent_logs(agent_id);

                PRAGMA user_version = 1;
                ",
            )
            .map_err(|e| RaError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn.lock().unwrap();

        // Verify tables exist
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='executions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='checkpoints'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_logs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migration_idempotent() {
        let db = Database::open_in_memory().unwrap();
        // Running migrate again should not fail
        db.migrate().unwrap();
    }
}
