use std::collections::{HashMap, HashSet};

use crate::{
    error::QueryError,
    helpers::{Column, QueryResult, RecordType, SqliteSchema},
    sql::{Condition, Ident, StringLit},
};

pub struct Plan {
    pub indexed_conditions: Vec<(usize, Column)>,
    pub scan_conditions: Vec<(usize, Column)>,
}

pub fn plan(
    conditions: Vec<Condition>,
    parsed_columns: &HashMap<Ident, usize>,
    index_schemas: &[&SqliteSchema],
) -> QueryResult<Plan> {
    let indexed: HashSet<usize> = index_schemas
        .iter()
        .filter_map(|s| match s.ty() {
            RecordType::Index { col_name } => parsed_columns.get(col_name).copied(),
            RecordType::Table { .. } => None,
        })
        .collect();

    let (indexed_conditions, scan_conditions) = conditions
        .into_iter()
        .map(|c| {
            let idx = *parsed_columns
                .get(&c.column_name)
                .ok_or_else(|| QueryError::NoSuchColumn(c.column_name.clone()))?;
            Ok((idx, c.exp_value))
        })
        .collect::<QueryResult<Vec<_>>>()?
        .into_iter()
        .partition(|(idx, _)| indexed.contains(idx));

    Ok(Plan {
        indexed_conditions,
        scan_conditions,
    })
}
