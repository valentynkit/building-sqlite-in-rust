mod cli;
mod commands;
pub mod config;
mod constants;

use std::fs::File;

use anyhow::bail;
pub use cli::*;
use tracing::{debug, info};

use crate::commands::{
    dbinfo,
    helpers::{SqliteSchema, db_header, page_header, parse_cell, read_page},
    sql_query, tables,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = cli.db_path;
    let cmd = cli.cmd;
    let mut file = File::open(path)?;
    let db_hdr = db_header(&file)?;

    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0)?;
    let schemas = parse_sqlite_schemas(&page_buf)?;
    let output = match cmd {
        Command::DbInfo => dbinfo::run(&page_buf, db_hdr),
        Command::Tables => tables::run(schemas),
        Command::SqlQuery(query) => sql_query::run(&file, schemas, db_hdr, query),
        _ => bail!("Missing or invalid command passed: {}", cmd),
    }?;

    println!("{output}");
    Ok(())
}

fn parse_sqlite_schemas(page_buf: &[u8]) -> anyhow::Result<Vec<SqliteSchema>> {
    let page_header = page_header(&page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        debug!(offset = ptr, "start parsing cell");
        let (cell, _) = parse_cell(&page_buf[(ptr as usize)..])?;
        schemas.push(SqliteSchema::parse(cell.record.values)?);
    }

    Ok(schemas)
}
