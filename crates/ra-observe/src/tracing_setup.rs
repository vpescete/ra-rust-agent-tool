use ra_core::config::ObservabilityConfig;
use tracing_subscriber::EnvFilter;

pub fn init(config: &ObservabilityConfig) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false);

    if config.log_format == "json" {
        builder.json().init();
    } else {
        builder.init();
    }
}
