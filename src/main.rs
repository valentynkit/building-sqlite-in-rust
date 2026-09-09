use clap::Parser;
use codecrafters_sqlite::config::AppConfig;
use codecrafters_sqlite::{Cli, run};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt::time};

fn main() -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    logging_init(config.log_filter());
    info!(environment = config.environment().as_str(), "starting");
    let cli = Cli::parse();
    let db_path = &cli.db_path;
    let cmd = &cli.cmd;
    info!(?db_path, ?cmd, "cli parsed");
    run(cli)
}

fn logging_init(default_filter: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            // RUST_LOG still wins, so you can raise verbosity without editing config.
            EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter.into()),
        )
        .with_timer(time::uptime())
        .with_writer(std::io::stderr)
        .init();
}
