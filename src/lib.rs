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
use tracing::debug;

use crate::{
    commands::{dbinfo, sql_query, tables},
    helpers::{db_header, read_page},
    sql::parse_sqlite_schemas,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;
    let file = File::open(&path).with_context(|| format!("opening {path}"))?;
    let db_hdr = db_header(&file)?;

    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0)?;
    let schemas = parse_sqlite_schemas(&page_buf)?;
    let page_size = db_hdr.page_size();
    let output = match cmd {
        Command::DbInfo => dbinfo::run(&page_buf, db_hdr)?,
        Command::Tables => tables::run(&schemas),
        Command::SqlQuery(query) => sql_query::run(&file, &schemas, page_size, &query)?,
    };

    println!("{output}");
    Ok(())
}
