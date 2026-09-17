use std::{
    collections::{HashMap, HashSet},
    fmt::format,
    fs::File,
};

use anyhow::bail;
use tracing::{debug, error, info, warn};

use crate::{
    commands::helpers::{
        Column, DbHeader, PageType, RecordType, SqliteSchema, TableLeafCell, page_header,
        parse_leaf_cell, read_page,
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

/// Splits on whitespace, except a single-quoted literal stays one token, quotes included.
/// `a = 'New York'` -> ["a", "=", "'New York'"]
fn tokenize(query: &str) -> Result<Vec<&str>> {
    let mut tokens = vec![];
    let mut rest = query.trim_start();
    while !rest.is_empty() {
        let end = if rest.starts_with('\'') {
            let close = rest[1..].find('\'').ok_or_else(|| QueryError::Malformed {
                query: query.to_owned(),
                reason: "unterminated string literal".to_owned(),
            })?;
            close + 2 // past the opening and closing quote
        } else {
            rest.find(char::is_whitespace).unwrap_or(rest.len())
        };
        tokens.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    Ok(tokens)
}

fn walk_index(
    file: &File,
    page_num: usize,
    keep: &dyn Fn(&TableLeafCell) -> bool,
    cells: &mut Vec<TableLeafCell>,
    db_hdr: &DbHeader,
    table: &SqliteSchema,
) -> Result<()> {
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Index { col_name } = table.ty() else {
        error!("expected to have RecordType::Index");
        return Err(QueryError::NoSuchTable(
            "<Couldn't get tbl_name>".to_string(),
        ));
    };

    match page_type {
        PageType::LeafIndex => {
            for &ptr in page_header.cell_pointers() {
                let (mut cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
                cells.push(cell);
            }
        }
        PageType::InteriorIndex => {
            for &ptr in page_header.cell_pointers() {
                let ptr = ptr as usize;
                let child = (u32::from_be_bytes(page_buf[ptr..ptr + 4].try_into()?) - 1) as usize;
                walk_index(file, child, keep, cells, db_hdr, table)?;
            }
            let Some(right_child) = page_header.right_most_child() else {
                error!("no right child found for InteriorIndex");
                return Err(FormatError::PageType(PageType::InteriorIndex.into()).into());
            };
            walk_index(file, (right_child - 1) as usize, keep, cells, db_hdr, table)?;
        }
        PageType::LeafTable | PageType::InteriorTable => {
            unimplemented!("index traversing unimplemented!")
        }
    }
    Ok(())
}

fn walk(
    file: &File,
    page_num: usize,
    keep: &dyn Fn(&TableLeafCell) -> bool,
    cells: &mut Vec<TableLeafCell>,
    db_hdr: &DbHeader,
    table: &SqliteSchema,
) -> Result<()> {
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Table {
        parsed_columns: _,
        rowid_alias,
    } = table.ty()
    else {
        return Err(QueryError::NoSuchTable(
            "<Couldn't get tbl_name>".to_string(),
        ));
    };

    match page_type {
        PageType::LeafTable => {
            for &ptr in page_header.cell_pointers() {
                let (mut cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
                if let Some(v) = rowid_alias.and_then(|i| cell.record.values.get_mut(i)) {
                    *v = Column::Int(cell.rowid);
                }
                if keep(&cell) {
                    cells.push(cell);
                }
            }
        }
        PageType::InteriorTable => {
            for &ptr in page_header.cell_pointers() {
                let ptr = ptr as usize;
                let child = (u32::from_be_bytes(page_buf[ptr..ptr + 4].try_into()?) - 1) as usize;
                walk(file, child, keep, cells, db_hdr, table)?;
            }
            let Some(right_child) = page_header.right_most_child() else {
                return Err(FormatError::PageType(PageType::InteriorTable.into()).into());
            };
            walk(file, (right_child - 1) as usize, keep, cells, db_hdr, table)?;
        }
        PageType::LeafIndex | PageType::InteriorIndex => {
            unimplemented!("index traversing unimplemented!")
        }
    }
    Ok(())
}

fn parse_table_and_index_schemas<'a>(
    schemas: &'a Vec<SqliteSchema>,
    tbl_name: &str,
) -> Result<(Option<&'a SqliteSchema>, Vec<&'a SqliteSchema>)> {
    // we may have several schema for the same tbl_name, for example:
    // 1 for table, and 3 for indexes for this table.
    let records: Vec<&SqliteSchema> = schemas
        .iter()
        .filter(|s| s.tbl_name().eq_ignore_ascii_case(tbl_name))
        .collect();

    if records.is_empty() {
        return Err(QueryError::NoSuchTable((tbl_name).to_owned()));
    }
    let mut table_schema: Option<&SqliteSchema> = None;
    let mut index_schemas: Vec<&SqliteSchema> = vec![];
    for record in records {
        match record.ty() {
            RecordType::Table {
                parsed_columns: _,
                rowid_alias: _,
            } => {
                if let Some(table_schema) = table_schema {
                    return Err(QueryError::DuplicatedTable(
                        table_schema.tbl_name().to_owned(),
                    ));
                }
                table_schema = Some(record);
            }
            RecordType::Index { col_name: _ } => {
                index_schemas.push(record);
            }
        }
    }
    Ok((table_schema, index_schemas))
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

    let tokens = tokenize(query)?;
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

    let (table_schema, index_schemas) = parse_table_and_index_schemas(&schemas, tbl_name)?;

    let Some(table_schema) = table_schema else {
        return Err(QueryError::NoSuchTable(tbl_name.to_owned()));
    };

    let RecordType::Table {
        parsed_columns,
        rowid_alias: _,
    } = table_schema.ty()
    else {
        return Err(QueryError::NoSuchTable(tbl_name.to_owned()));
    };

    debug!(?table_schema, ?index_schemas);
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(
        &file,
        &mut page_buf,
        page_size,
        table_schema.rootpage_index(),
    )?;
    let page_header = page_header(&page_buf, table_schema.rootpage_index())?;
    let mut cells: Vec<TableLeafCell> = Vec::with_capacity(page_header.cell_count() as usize);

    let conditions: Vec<(usize, &str)> = conditions
        .iter()
        .map(|(col, expected)| {
            parsed_columns
                .get(&col.to_ascii_lowercase())
                .map(|&idx| (idx, *expected))
                .ok_or_else(|| QueryError::NoSuchColumn((*col).to_owned()))
        })
        .collect::<Result<_>>()?;

    let indexed: HashSet<usize> = index_schemas
        .iter()
        .filter_map(|s| match s.ty() {
            RecordType::Index { col_name } => parsed_columns.get(col_name).copied(),
            _ => None,
        })
        .collect();

    let (indexed_conditions, scan_conditions): (Vec<_>, Vec<_>) = conditions
        .into_iter()
        .partition(|(idx, _)| indexed.contains(idx));

    let keep = |cell: &TableLeafCell| {
        scan_conditions.iter().all(|&(idx, expected)| {
            cell.record
                .values
                .get(idx)
                .is_some_and(|v| v.to_string() == expected)
        })
    };

    let keep_indexes = |cell: &TableLeafCell| {
        indexed_conditions.iter().all(|&(idx, expected)| {
            cell.record
                .values
                .get(idx)
                .is_some_and(|v| v.to_string() == expected)
        })
    };
    let mut index_cells: Vec<TableLeafCell> = vec![];

    /// TODO: for sicplicity we just handle first index for now, without composite indexes etc...
    if !indexed_conditions.is_empty() {
        // TODO: we are also not handling that index_schemas may contain indexes that doesn't exist
        // in conditions, ideally we should derive it from indexed_conditions
        let index = index_schemas[0];
        // traversing the indexes
        walk_index(
            file,
            index.rootpage_index(),
            &keep_indexes,
            &mut index_cells,
            &db_hdr,
            table_schema,
        )?;

        todo!("use indexes cells to walk through and filter on remaining conditions");
    } else {
        walk(
            file,
            table_schema.rootpage_index(),
            &keep,
            &mut cells,
            &db_hdr,
            table_schema,
        )?;
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

        let Some(&t_idx) = parsed_columns.get(&col) else {
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

#[cfg(test)]
mod tests {
    use super::tokenize;

    #[test]
    fn tokenize_keeps_quoted_literal_whole() {
        assert_eq!(
            tokenize("  name from t where city = 'New York'  ").unwrap(),
            ["name", "from", "t", "where", "city", "=", "'New York'"]
        );
        assert_eq!(tokenize("a\t'x'\n").unwrap(), ["a", "'x'"]);
        assert!(tokenize("a = 'oops").is_err());
    }
}
