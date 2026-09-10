use std::{fmt::format, fs::File};

use anyhow::bail;
use tracing::{debug, info, warn};

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
    let query = (&query[0]).to_owned().to_ascii_lowercase();

    info!(%query, "executing sql");
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.clone(),
        reason: reason.to_owned(),
    };

    let query = query
        .strip_prefix("select")
        .ok_or(malformed("expected `SELECT <...>"))?;

    let tokens: Vec<&str> = query.split_whitespace().collect();
    let [what @ .., from, tbl_name] = tokens.as_slice() else {
        return Err(malformed("expected `SELECT <expr> FROM <table>`"));
    };

    if !from.eq_ignore_ascii_case("from") {
        return Err(malformed("expected SELECT"));
    }

    let table = schemas
        .iter()
        .find(|s| s.tbl_name() == *tbl_name)
        .ok_or(QueryError::NoSuchTable((*tbl_name).to_owned()))?;

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

    if what.len() == 1 && what[0].eq_ignore_ascii_case("count(*)") {
        return Ok(cells.len().to_string());
    }

    let mut out: Vec<String> = vec![String::new(); cells.len() * what.len()];
    for (idx_col, &col) in what.iter().enumerate() {
        let col = col
            .to_ascii_lowercase()
            .trim_matches(|c: char| c.is_whitespace() || c == ',')
            .to_owned();

        let Some(&t_idx) = table.sql_parsed().get(&col) else {
            return Err(QueryError::NoSuchColumn(col.to_owned()));
        };

        for (idx_row, row) in cells.iter().enumerate() {
            let value = row
                .record
                .values
                .get(t_idx)
                .ok_or(QueryError::NoSuchColumn(col.clone()))?;

            let idx = idx_col + (idx_row * what.len());
            out[idx] = value.to_string();
        }
    }

    let out = out
        .chunks(what.len())
        .map(|row| row.join("|"))
        .collect::<Vec<String>>()
        .join("\n");

    Ok(out)
}
