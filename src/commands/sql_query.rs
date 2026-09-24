use std::fs::File;

use tracing::{info, instrument};

use crate::{
    error::QueryError,
    helpers::{QueryResult, RecordType, SqliteSchema, btree_walk},
    sql::{
        ParsedTokens, Projection, filter_what_col, parse_query, plan, resolve_table_schemas,
        tokenize,
    },
};

#[instrument(level = "info", skip(schemas, file), ret, err)]
pub fn run(
    file: &File,
    schemas: &[SqliteSchema],
    page_size: u16,
    query: &[String],
) -> QueryResult<String> {
    let query = query.join(" ").trim_start().to_owned();

    info!(%query, "executing sql");
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.clone(),
        reason: reason.to_owned(),
    };

    let tokens = tokenize(&query)?;

    if tokens.len() <= 2 {
        return Err(malformed("expected query to have more than 4 words"));
    }

    let ParsedTokens {
        projection,
        table,
        conditions,
    } = parse_query(tokens)?;

    let table_schemas = resolve_table_schemas(schemas, &table)?;

    let RecordType::Table {
        parsed_columns,
        rowid_alias: _,
    } = table_schemas.table.ty()
    else {
        return Err(QueryError::NoSuchTable(table));
    };

    let walk_plan = plan(conditions, parsed_columns, &table_schemas)?;

    /*
        let keep_indexes = |cell: &TableLeafCell| {
            indexed_conditions.iter().all(|&(idx, expected)| {
                cell.record
                    .values
                    .get(idx)
                    .is_some_and(|v| v.to_string() == expected)
            })
        };
    */

    let cells = btree_walk(file, walk_plan, table_schemas, page_size)?;
    let out = match projection {
        Projection::Count => cells.len().to_string(),
        Projection::Columns(columns) => filter_what_col(&cells, &columns, parsed_columns)?,
    };
    Ok(out)
}
