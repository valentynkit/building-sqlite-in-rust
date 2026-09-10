use std::fs::File;

use anyhow::bail;
use tracing::{debug, info};

use crate::{
    commands::helpers::{
        DbHeader, SqliteSchema, TableLeafCell, page_header, parse_cell, read_page,
    },
    error::{FormatError, QueryError},
};

pub(crate) fn run(
    file: &File,
    schemas: Vec<SqliteSchema>,
    db_hdr: DbHeader,
    query: Vec<String>,
) -> Result<String, QueryError> {
    assert_eq!(query.len(), 1, "query should be one element");
    let query = (&query[0]).to_owned();

    info!(%query, "executing sql");
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.clone(),
        reason: reason.to_owned(),
    };

    let tokens: Vec<&str> = query.split_whitespace().collect();
    let &[select, what, .., from, tbl_name] = tokens.as_slice() else {
        return Err(malformed("expected `SELECT <expr> FROM <table>`"));
    };
    if !select.eq_ignore_ascii_case("select") {
        return Err(malformed("expected SELECT"));
    }

    if !from.eq_ignore_ascii_case("from") {
        return Err(malformed("expected SELECT"));
    }

    let table = schemas
        .iter()
        .find(|s| s.tbl_name() == tbl_name)
        .ok_or(QueryError::NoSuchTable(tbl_name.to_owned()))?;

    debug!(?table);
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, table.rootpage_index())?;
    let page_header = page_header(&page_buf, table.rootpage_index())?;
    let mut cells: Vec<TableLeafCell> = Vec::with_capacity(page_header.cell_count() as usize);

    for &ptr in page_header.cell_pointers() {
        let (cell, _) = parse_cell(&page_buf[(ptr as usize)..])?;
        cells.push(cell);
    }

    let out = match what.to_ascii_lowercase().as_str() {
        "count(*)" => format!("{}", page_header.cell_count()),
        s => {
            let Some(idx) = table.sql_parsed().get(what) else {
                return Err(QueryError::NoSuchColumn(s.to_owned()));
            };
            let mut values: Vec<String> = vec![];
            for cell in cells {
                let value = cell
                    .record
                    .values
                    .get(*idx)
                    .ok_or(QueryError::NoSuchColumn(format!("{idx}")))?;
                values.push(value.to_string());
            }
            values.join("\n")
        }
    };

    Ok(out)
}
