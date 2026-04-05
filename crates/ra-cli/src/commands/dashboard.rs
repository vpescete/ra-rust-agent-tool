use ra_core::RuntimeConfig;

pub async fn execute(_config: &RuntimeConfig) -> anyhow::Result<()> {
    eprintln!("Connecting to RA MCP server via IPC socket...");

    let collector = ra_observe::connect_and_collect(None).await?;

    eprintln!("Connected. Dashboard starting.");

    ra_observe::run_dashboard(collector).await
}
