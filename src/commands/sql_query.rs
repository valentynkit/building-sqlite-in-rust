use std::{
    collections::{HashMap, HashSet},
    fs::File,
};

use tracing::{debug, info, warn};

use crate::{
    error::{FormatError, QueryError},
    helpers::{QueryResult, RecordType, SqliteSchema, TableLeafCell, walk, walk_index},
    sql::{
        Ident, ParsedTokens, Plan, filter_what_col, get_columns_schemas, parse_query,
        parse_table_and_index_schemas, tokenize,
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
    let tbl_name = Ident::from(from[0]);

    let (table_schema, index_schemas) = parse_table_and_index_schemas(schemas, tbl_name)?;

    let Some(table_schema) = table_schema else {
        return Err(QueryError::NoSuchTable(tbl_name));
    };

    let RecordType::Table {
        parsed_columns,
        rowid_alias: _,
    } = table_schema.ty()
    else {
        return Err(QueryError::NoSuchTable(tbl_name));
    };

    debug!(?table_schema, ?index_schemas);
    let mut cells: Vec<TableLeafCell> = vec![];

    let Plan {
        indexed_conditions,
        scan_conditions,
    } = get_columns_schemas(&conditions, parsed_columns, &index_schemas)?;

    let keep = |cell: &TableLeafCell| {
        scan_conditions.iter().all(|&(idx, expected)| {
            cell.record
                .values
                .get(idx)
                .is_some_and(|v| v.to_string() == expected)
        })
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
            table_schema.rootpage_index()?,
            table_schema.ty(),
            &keep,
            &mut cells,
        )?;
    } else {
        // TODO: we are also not handling that index_schemas may contain indexes that doesn't exist
        // in conditions, ideally we should derive it from indexed_conditions
        let index_schema = index_schemas[0];

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

    if what.len() == 1 && what[0].eq_ignore_ascii_case("count(*)") {
        return Ok(cells.len().to_string());
    }

    let out = filter_what_col(&cells, &what, parsed_columns)?;
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
