use std::collections::HashMap;

use tracing::debug;

use crate::{
    error::FormatError,
    helpers::{Column, RecordType, Result, SqliteSchema, int, page_header, parse_leaf_cell, text},
};

fn parse_index_sql(
    query: &str,
    uknown_sql: &impl Fn(&str, &str) -> FormatError,
) -> Result<RecordType> {
    let open = query.find('(').ok_or_else(|| uknown_sql("(", ""))?;
    let close = query.find(')').ok_or_else(|| uknown_sql(")", ""))?;

    if open >= close {
        return Err(uknown_sql(
            "(...)",
            &format!("`(` at {open} after `)` at {close}"),
        ));
    }

    let inside_parentheses = &query[open + 1..close];
    Ok(RecordType::Index {
        col_name: inside_parentheses.to_owned(),
    })
}

fn parse_table_sql(
    query: &str,
    uknown_sql: &impl Fn(&str, &str) -> FormatError,
) -> Result<RecordType> {
    let open = query.find('(').ok_or_else(|| uknown_sql("(", ""))?;
    let close = query.find(')').ok_or_else(|| uknown_sql(")", ""))?;

    if open >= close {
        return Err(uknown_sql(
            "(...)",
            &format!("`(` at {open} after `)` at {close}"),
        ));
    }

    let inside_parentheses = &query[open + 1..close];

    debug!(?inside_parentheses);
    let mut parsed_columns: HashMap<String, usize> = HashMap::new();
    let mut rowid_alias = None;
    let columns: Vec<&str> = inside_parentheses.split(',').map(str::trim).collect();

    debug!(?columns);
    for (idx, sub_str) in columns.iter().enumerate() {
        let item = sub_str
            .split_whitespace()
            .next()
            .ok_or(FormatError::UknownSql {
                expected: "column name".to_owned(),
                got: "None".to_owned(),
            })?;
        parsed_columns.insert(item.to_owned(), idx);
        if sub_str.contains("integer primary key") {
            rowid_alias = Some(idx);
        }
    }

    debug!(?parsed_columns, ?rowid_alias);
    let rec = RecordType::Table {
        parsed_columns,
        rowid_alias,
    };

    Ok(rec)
}

pub(crate) fn parse(values: Vec<Column>) -> Result<SqliteSchema> {
    let [ty, _, tbl_name, rootpage, sql_query]: [Column; 5] =
        values
            .try_into()
            .map_err(|v: Vec<Column>| FormatError::Schema {
                exp_len: 5,
                actual_len: v.len(),
            })?;

    let sql_query = text(4, sql_query)?;
    let tbl_name = text(2, tbl_name)?;

    let query = sql_query.to_ascii_lowercase();

    let uknown_sql = |expected: &str, got: &str| FormatError::UknownSql {
        expected: expected.to_owned(),
        got: got.to_owned(),
    };

    let query_start = format!("create {ty}");
    if !query.starts_with(query_start.as_str()) {
        return Err(FormatError::UknownSql {
            got: query,
            expected: query_start,
        });
    }
    debug!(?query, ?tbl_name, "parsed sql");

    let ty = match text(0, ty)?.as_str() {
        "table" => parse_table_sql(&query, &uknown_sql),
        "index" => parse_index_sql(&query, &uknown_sql),
        str => Err(FormatError::UknownRecordType(str.to_owned())),
    }?;

    let schema = SqliteSchema::new(ty, tbl_name, int(3, rootpage)?);
    /*
                name: text(1, name)?,
                sql_query,
    */

    debug!(?schema, "parsed");

    Ok(schema)
}

pub fn parse_sqlite_schemas(page_buf: &[u8]) -> anyhow::Result<Vec<SqliteSchema>> {
    let page_header = page_header(page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        debug!(offset = ptr, "start parsing cell");
        let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
        schemas.push(parse(cell.record.values)?);
    }

    Ok(schemas)
}
