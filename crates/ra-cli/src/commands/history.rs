use ra_core::RuntimeConfig;
use ra_store::{Database, HistoryStore};

pub async fn execute(limit: usize, config: &RuntimeConfig) -> anyhow::Result<()> {
    let db_path = ra_core::config::RuntimeConfig::expand_path(&config.persistence.db_path);

    if !std::path::Path::new(&db_path).exists() {
        println!("No execution history yet.");
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let store = HistoryStore::new(db.conn.clone());

    let executions = store.list_executions(limit).await?;

    if executions.is_empty() {
        println!("No execution history yet.");
        return Ok(());
    }

    println!(
        "{:<36}  {:<20}  {:<12}  {:<10}  {:<10}",
        "ID", "Name", "Status", "Cost", "Tokens"
    );
    println!("{}", "-".repeat(92));

    for exec in &executions {
        println!(
            "{:<36}  {:<20}  {:<12}  ${:<9.4}  {:<10}",
            exec.id, exec.name, exec.status, exec.total_cost_usd, exec.total_tokens
        );
    }

    println!("\n{} execution(s) shown.", executions.len());

    Ok(())
}
