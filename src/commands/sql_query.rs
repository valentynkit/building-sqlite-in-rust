use std::fs::File;

use tracing::{debug, info};

use crate::{
    error::QueryError,
    helpers::{QueryResult, RecordType, SqliteSchema, TableLeafCell, walk, walk_index},
    sql::{
        Ident, ParsedTokens, Plan, TableSchemas, filter_what_col, parse_query, plan,
        resolve_table_schemas, tokenize,
    },
};

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
        what,
        from,
        conditions,
    } = parse_query(&query, tokens)?;

    if what.is_empty() {
        return Err(malformed("expected SELECT <expr> "));
    }
    if from.len() != 1 {
        return Err(malformed("expected FROM <tbl_name> "));
    }

    let tbl_name = from[0].clone();

    let TableSchemas { table, indexes } = resolve_table_schemas(schemas, &tbl_name)?;

    let RecordType::Table {
        parsed_columns,
        rowid_alias: _,
    } = table.ty()
    else {
        return Err(QueryError::NoSuchTable(tbl_name));
    };

    debug!(?table, ?indexes);
    let mut cells: Vec<TableLeafCell> = vec![];

    let Plan {
        indexed_conditions,
        scan_conditions,
    } = plan(conditions, parsed_columns, &indexes)?;

    let keep = |cell: &TableLeafCell| {
        scan_conditions
            .iter()
            .all(|(idx, expected)| cell.record.values.get(*idx) == Some(expected))
    };

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

    let mut index_cells: Vec<TableLeafCell> = vec![];

    // TODO: for sicplicity we just handle first index for now, without composite indexes etc...
    if indexed_conditions.is_empty() {
        walk(
            file,
            page_size,
            table.rootpage_index()?,
            table.ty(),
            &keep,
            &mut cells,
        )?;
    } else {
        // TODO: we are also not handling that index_schemas may contain indexes that doesn't exist
        // in conditions, ideally we should derive it from indexed_conditions
        let index_schema = indexes[0];

        // traversing the indexes
        walk_index(
            file,
            page_size,
            index_schema.rootpage_index()?,
            index_schema.ty(),
            &mut index_cells,
        )?;

        todo!("use indexes cells to walk through and filter on remaining conditions");
    }

    if what.len() == 1 && what[0] == "count(*)".to_string() {
        return Ok(cells.len().to_string());
    }

    let out = filter_what_col(&cells, &what, parsed_columns)?;
    Ok(out)
}
