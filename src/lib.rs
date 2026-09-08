mod cli;
mod commands;
pub mod config;
mod constants;

use std::fs::File;

use anyhow::bail;
pub use cli::*;
use tracing::info;

use crate::commands::{dbinfo, helpers::db_header, tables};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;
    let mut file = File::open(path)?;
    let db_header = db_header(&file)?;

    match cmd {
        Command::DbInfo => {
            dbinfo::run(&file, db_header)?;
        }
        Command::Tables => {
            tables::run(&file, db_header)?;
        }

        _ => bail!("Missing or invalid command passed: {}", cmd),
    }

    Ok(())
}
