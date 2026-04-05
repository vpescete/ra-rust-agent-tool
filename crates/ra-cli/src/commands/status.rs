use ra_core::RuntimeConfig;

pub async fn execute(_config: &RuntimeConfig) -> anyhow::Result<()> {
    // In a full implementation, this would connect to a running daemon
    // or read state from a shared file/socket.
    // For now, show that no daemon is running.
    println!("RA Status");
    println!("=========");
    println!("No active workflow runner detected.");
    println!("Use 'ra run <workflow.yaml>' to start a workflow.");
    println!("Use 'ra dashboard' during execution to monitor agents.");
    Ok(())
}
