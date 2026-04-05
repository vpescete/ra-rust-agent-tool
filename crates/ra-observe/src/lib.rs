pub mod dashboard;
pub mod export;
pub mod ipc_client;
pub mod metrics_collector;
pub mod tracing_setup;

pub use dashboard::run_dashboard;
pub use ipc_client::connect_and_collect;
pub use metrics_collector::{DashboardData, LogEntry, LogEventType, MetricsCollector};
