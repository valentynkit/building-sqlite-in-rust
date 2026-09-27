mod cli;
mod commands;
pub mod config;
mod constants;
mod error;
mod helpers;
mod sql;

use anyhow::Context;
pub use cli::*;

use crate::{
    commands::{dbinfo, sql_query, tables},
    helpers::{Database, parse_sqlite_schemas},
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;
    let database = Database::open(&path)?;
    let schemas =
        parse_sqlite_schemas(&database).with_context(|| format!("loading the schema of {path}"))?;
    let output = match cmd {
        Command::DbInfo => dbinfo::run(&database, &schemas),
        Command::Tables => tables::run(&schemas),
        Command::SqlQuery(query) => sql_query::run(&database, &schemas, &query)
            .with_context(|| format!("executing `{}`", query.join(" ")))?,
    };

    // An empty result prints nothing, as sqlite3 does, not a blank line.
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(())
}
