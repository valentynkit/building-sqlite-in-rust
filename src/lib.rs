mod cli;
mod commands;
pub mod config;
mod constants;
mod error;
mod helpers;
mod sql;
use std::fs::File;

use anyhow::Context;
pub use cli::*;

use crate::{
    commands::{dbinfo, sql_query, tables},
    sql::{parse_first_page, parse_sqlite_schemas},
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;
    let file = File::open(&path).with_context(|| format!("opening {path}"))?;
    let first_page =
        parse_first_page(&file).with_context(|| format!("reading the first page of {path}"))?;
    let page_size = u16::try_from(first_page.len())?;
    let schemas = parse_sqlite_schemas(&first_page)
        .with_context(|| format!("loading the schema of {path}"))?;
    let output = match cmd {
        Command::DbInfo => dbinfo::run(&first_page)?,
        Command::Tables => tables::run(&schemas),
        Command::SqlQuery(query) => sql_query::run(&file, &schemas, page_size, &query)
            .with_context(|| format!("executing `{}`", query.join(" ")))?,
    };

    println!("{output}");
    Ok(())
}
