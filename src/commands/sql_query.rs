use std::{collections::HashMap, fmt::format, fs::File};

use anyhow::bail;
use tracing::{debug, info, warn};

use crate::{
    commands::helpers::{
        DbHeader, SqliteSchema, TableLeafCell, page_header, parse_cell, read_page,
    },
    error::{FormatError, QueryError},
};

pub type Result<T, E = QueryError> = core::result::Result<T, E>;

/// Used for parsing, during iteration to identify what is the current query section.
#[derive(Debug, PartialEq, Eq)]
enum QuerySection {
    Select,
    From,
    Where,
}

impl QuerySection {
    // State machine, moving to next query section
    fn next(self, query: &str) -> Result<Self> {
        match self {
            Self::Select => Ok(Self::From),
            Self::From => Ok(Self::Where),
            Self::Where => Err(QueryError::Malformed {
                query: query.to_owned(),
                reason: "WHERE is the last query section".to_owned(),
            }),
        }
    }

    fn validate(&self, query: &str, token: &str) -> Result<()> {
        let (section, forbidden): (&str, &[&str]) = match self {
            Self::Select => ("SELECT", &["select", "where"]),
            Self::From => ("FROM", &["select", "from"]),
            Self::Where => ("WHERE", &["select", "from", "where"]),
        };
        if forbidden.contains(&token.to_ascii_lowercase().as_str()) {
            return Err(QueryError::Malformed {
                query: query.to_owned(),
                reason: format!("{section} section shouldn't contain {token}"),
            });
        }
        Ok(())
    }
}

fn parse_query<'a>(
    query: &'a str,
    tokens: &[&'a str],
    what: &mut Vec<&'a str>,
    from: &mut Vec<&'a str>,
    conditions: &mut Vec<(&'a str, &'a str)>,
) -> Result<()> {
    let mut query_section = QuerySection::Select;
    let mut idx: i32 = -1;

    let malformed = |reason: &str| QueryError::Malformed {
        query: query.to_owned(),
        reason: reason.to_owned(),
    };

    for &token in tokens {
        idx += 1;
        query_section.validate(query, token)?;
        match query_section {
            QuerySection::Select => {
                if token.eq_ignore_ascii_case("from") {
                    if what.is_empty() {
                        return Err(malformed(
                            "expected > 0 items after SELECT, got 0. `SELECT FROM ...`",
                        ));
                    }
                    query_section = query_section.next(query)?;
                    continue;
                }
                what.push(token);
            }
            QuerySection::From => {
                if token.eq_ignore_ascii_case("where") {
                    if from.is_empty() {
                        return Err(malformed(
                            "expected > 0 items after FROM, got 0. `SELECT <...> FROM WHERE`",
                        ));
                    }
                    query_section = query_section.next(query)?;
                    idx += 1;
                    break;
                }
                from.push(token);
            }
            QuerySection::Where => {
                return Err(malformed("unknown section during parsing"));
            }
        }
    }

    if query_section == QuerySection::Where {
        if idx as usize == &tokens.len() - 1 {
            return Err(malformed(
                "expected `SELECT <expr> FROM <table> WHERE <condition>`, has missing conditions after WHERE",
            ));
        }
        for chunk in tokens[(idx as usize)..].chunks(3) {
            let [col, condition, value] = chunk else {
                return Err(malformed("WHERE expects `<col> <op> <value>`"));
            };
            warn!(?col, ?condition, ?value, "parsed query chunk");
            if *condition != "=" {
                return Err(malformed("WHERE expects `<col> = <value>`"));
            }

            conditions.push((col, value.trim_matches('\'')));

            debug!(?col, ?condition, ?value, "parsing WHERE section");
        }
    }

    debug!(?what, ?from, ?idx, ?query_section, "parsed SQL query");
    Ok(())
}

pub(crate) fn run(
    file: &File,
    schemas: Vec<SqliteSchema>,
    db_hdr: DbHeader,
    query: Vec<String>,
) -> Result<String> {
    let query = query.join(" ");

    info!(%query, "executing sql");
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.clone(),
        reason: reason.to_owned(),
    };

    // Case matters inside string literals, so only the keyword is matched case-insensitively.
    let query = query
        .get(..6)
        .filter(|kw| kw.eq_ignore_ascii_case("select"))
        .map(|_| &query[6..])
        .ok_or_else(|| malformed("expected `SELECT <...>`"))?;

    let tokens: Vec<&str> = query.split_whitespace().collect();
    let mut what: Vec<&str> = vec![];
    let mut from: Vec<&str> = vec![];
    // could be empty, column = expected value.
    let mut conditions: Vec<(&str, &str)> = vec![];

    if tokens.len() <= 2 {
        return Err(malformed("expected query to have more than 4 words"));
    }

    parse_query(query, &tokens, &mut what, &mut from, &mut conditions)?;
    if what.is_empty() {
        return Err(malformed("expected SELECT <expr> "));
    }
    if from.len() != 1 {
        return Err(malformed("expected FROM <tbl_name> "));
    }
    let tbl_name = from[0];

    let table = schemas
        .iter()
        .find(|s| s.tbl_name().eq_ignore_ascii_case(tbl_name))
        .ok_or(QueryError::NoSuchTable((tbl_name).to_owned()))?;

    debug!(?table);
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, table.rootpage_index())?;
    let page_header = page_header(&page_buf, table.rootpage_index())?;
    let mut cells: Vec<TableLeafCell> = Vec::with_capacity(page_header.cell_count() as usize);

    for &ptr in page_header.cell_pointers() {
        let (cell, _) = parse_cell(&page_buf[(ptr as usize)..])?;
        // Every condition must hold (AND); no conditions keeps the row.
        let mut meet_conditions = true;
        for (col, expected_value) in &conditions {
            let Some(&col_idx) = table.sql_parsed().get(&col.to_ascii_lowercase()) else {
                return Err(QueryError::NoSuchColumn((*col).to_owned()));
            };

            let actual_value = cell
                .record
                .values
                .get(col_idx)
                .ok_or(QueryError::NoSuchColumn((*col).to_owned()))?;

            if &actual_value.to_string() != expected_value {
                meet_conditions = false;
                break;
            }
        }
        if meet_conditions {
            cells.push(cell);
        }
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
