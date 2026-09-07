use clap::Parser;
use codecrafters_sqlite::{Cli, run};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run(cli)
}
