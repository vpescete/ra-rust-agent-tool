use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use ra_core::event::StreamEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetStatus {
    Ok,
    Warning { usage_pct: u32 },
    Exceeded,
}

struct TokenBudget {
    limit: u64,
    consumed: u64,
    warning_threshold: f64,
}

/// Shared key-value context for cross-agent data sharing
pub struct SharedContext {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl SharedContext {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set a key-value pair
    pub async fn set(&self, key: String, value: String) {
        self.data.write().await.insert(key, value);
    }

    /// Get a value by key
    pub async fn get(&self, key: &str) -> Option<String> {
        self.data.read().await.get(key).cloned()
    }

    /// Get all current entries
    pub async fn snapshot(&self) -> HashMap<String, String> {
        self.data.read().await.clone()
    }

    /// Merge multiple entries at once
    pub async fn merge(&self, entries: HashMap<String, String>) {
        let mut data = self.data.write().await;
        data.extend(entries);
    }

    /// Clear all data
    pub async fn clear(&self) {
        self.data.write().await.clear();
    }
}

impl Default for SharedContext {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ContextManager {
    budgets: Arc<RwLock<HashMap<Uuid, TokenBudget>>>,
    pub shared: SharedContext,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            budgets: Arc::new(RwLock::new(HashMap::new())),
            shared: SharedContext::new(),
        }
    }

    pub async fn register(&self, agent_id: Uuid, limit: u64) {
        self.budgets.write().await.insert(
            agent_id,
            TokenBudget {
                limit,
                consumed: 0,
                warning_threshold: 0.8,
            },
        );
    }

    pub async fn update(&self, agent_id: Uuid, event: &StreamEvent) {
        if let StreamEvent::Result { usage, .. } = event {
            if let Some(u) = usage {
                let total = u.input_tokens + u.output_tokens;
                let mut budgets = self.budgets.write().await;
                if let Some(budget) = budgets.get_mut(&agent_id) {
                    budget.consumed = total;
                }
            }
        }
    }

    pub async fn check_budget(&self, agent_id: Uuid) -> BudgetStatus {
        let budgets = self.budgets.read().await;
        match budgets.get(&agent_id) {
            Some(budget) => {
                if budget.limit == 0 {
                    return BudgetStatus::Ok;
                }
                let pct = budget.consumed as f64 / budget.limit as f64;
                if pct >= 1.0 {
                    BudgetStatus::Exceeded
                } else if pct >= budget.warning_threshold {
                    BudgetStatus::Warning {
                        usage_pct: (pct * 100.0) as u32,
                    }
                } else {
                    BudgetStatus::Ok
                }
            }
            None => BudgetStatus::Ok,
        }
    }

    pub async fn unregister(&self, agent_id: Uuid) {
        self.budgets.write().await.remove(&agent_id);
    }

    pub async fn get_consumed(&self, agent_id: Uuid) -> u64 {
        let budgets = self.budgets.read().await;
        budgets.get(&agent_id).map(|b| b.consumed).unwrap_or(0)
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_budget_ok() {
        let cm = ContextManager::new();
        let id = Uuid::new_v4();
        cm.register(id, 100_000).await;
        assert_eq!(cm.check_budget(id).await, BudgetStatus::Ok);
    }

    #[tokio::test]
    async fn test_budget_warning() {
        let cm = ContextManager::new();
        let id = Uuid::new_v4();
        cm.register(id, 100).await;
        {
            let mut budgets = cm.budgets.write().await;
            if let Some(b) = budgets.get_mut(&id) {
                b.consumed = 85;
            }
        }
        assert!(matches!(
            cm.check_budget(id).await,
            BudgetStatus::Warning { .. }
        ));
    }

    #[tokio::test]
    async fn test_budget_exceeded() {
        let cm = ContextManager::new();
        let id = Uuid::new_v4();
        cm.register(id, 100).await;
        {
            let mut budgets = cm.budgets.write().await;
            if let Some(b) = budgets.get_mut(&id) {
                b.consumed = 150;
            }
        }
        assert_eq!(cm.check_budget(id).await, BudgetStatus::Exceeded);
    }

    #[tokio::test]
    async fn test_no_budget_is_ok() {
        let cm = ContextManager::new();
        assert_eq!(cm.check_budget(Uuid::new_v4()).await, BudgetStatus::Ok);
    }

    // SharedContext tests
    #[tokio::test]
    async fn test_shared_context_set_get() {
        let ctx = SharedContext::new();
        ctx.set("key1".to_string(), "value1".to_string()).await;
        assert_eq!(ctx.get("key1").await, Some("value1".to_string()));
        assert_eq!(ctx.get("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_shared_context_snapshot() {
        let ctx = SharedContext::new();
        ctx.set("a".to_string(), "1".to_string()).await;
        ctx.set("b".to_string(), "2".to_string()).await;
        let snap = ctx.snapshot().await;
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.get("a"), Some(&"1".to_string()));
    }

    #[tokio::test]
    async fn test_shared_context_merge() {
        let ctx = SharedContext::new();
        ctx.set("existing".to_string(), "old".to_string()).await;

        let mut new_data = HashMap::new();
        new_data.insert("existing".to_string(), "updated".to_string());
        new_data.insert("new_key".to_string(), "new_val".to_string());
        ctx.merge(new_data).await;

        assert_eq!(ctx.get("existing").await, Some("updated".to_string()));
        assert_eq!(ctx.get("new_key").await, Some("new_val".to_string()));
    }

    #[tokio::test]
    async fn test_shared_context_clear() {
        let ctx = SharedContext::new();
        ctx.set("key".to_string(), "val".to_string()).await;
        ctx.clear().await;
        assert_eq!(ctx.get("key").await, None);
    }
}
