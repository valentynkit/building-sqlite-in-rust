mod cli;
mod commands;
pub mod config;
mod constants;

use anyhow::bail;
pub use cli::*;
use tracing::info;

use crate::commands::dbinfo;

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;

    match cmd {
        Command::DbInfo => {
            info!("executing db_info command");
            dbinfo::run(path)?;
        }
        _ => bail!("Missing or invalid command passed: {}", cmd),
    }

    Ok(())
}
