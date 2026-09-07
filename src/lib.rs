mod cli;
mod commands;
mod constants;

use anyhow::bail;
pub use cli::*;

use crate::commands::dbinfo;

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;

    match cmd {
        Command::DbInfo => {
            dbinfo::run(path);
        }
        _ => bail!("Missing or invalid command passed: {}", cmd),
    }

    Ok(())
}
