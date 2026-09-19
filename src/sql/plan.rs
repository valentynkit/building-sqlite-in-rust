use std::collections::{HashMap, HashSet};

use crate::{
    error::QueryError,
    helpers::{QueryResult, RecordType, SqliteSchema},
};

pub struct Plan<'a> {
    pub indexed_conditions: Vec<(usize, &'a str)>,
    pub scan_conditions: Vec<(usize, &'a str)>,
}

pub fn get_columns_schemas<'a>(
    conditions: Vec<Condition>,
    parsed_columns: &HashMap<String, usize>,
    index_schemas: &[&SqliteSchema],
) -> QueryResult<Plan<'a>> {
    let conditions: Vec<(usize, &str)> = conditions
        .iter()
        .map(|(col, expected)| {
            parsed_columns
                .get(&col.to_ascii_lowercase())
                .map(|&idx| (idx, *expected))
                .ok_or_else(|| QueryError::NoSuchColumn((*col).to_owned()))
        })
        .collect::<QueryResult<_>>()?;

    let indexed: HashSet<usize> = index_schemas
        .iter()
        .filter_map(|s| match s.ty() {
            RecordType::Index { col_name } => parsed_columns.get(col_name).copied(),
            RecordType::Table {
                parsed_columns: _,
                rowid_alias: _,
            } => None,
        })
        .collect();

    let (indexed_conditions, scan_conditions): (Vec<_>, Vec<_>) = conditions
        .into_iter()
        .partition(|(idx, _)| indexed.contains(idx));
    Ok(Plan {
        indexed_conditions,
        scan_conditions,
    })
}
