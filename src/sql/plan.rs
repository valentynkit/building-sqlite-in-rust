use std::collections::{HashMap, HashSet};

use tracing::instrument;

use crate::{
    error::{QueryError, QueryResult},
    helpers::{Column, RecordType},
    sql::{Condition, Ident, TableSchemas},
};

pub struct Plan {
    pub indexed: Vec<(usize, Column)>,
    pub normal: Vec<(usize, Column)>,
}

#[instrument(level = "debug", skip(parsed_columns, conditions, table_schemas))]
pub fn plan(
    conditions: Vec<Condition>,
    parsed_columns: &HashMap<Ident, usize>,
    table_schemas: &TableSchemas,
) -> QueryResult<Plan> {
    let indexed: HashSet<usize> = table_schemas
        .indexes
        .iter()
        .filter_map(|s| match s.ty() {
            RecordType::Index { col_name } => parsed_columns.get(col_name).copied(),
            RecordType::Table { .. } => None,
        })
        .collect();

    let (indexed, normal) = conditions
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

    Ok(Plan { indexed, normal })
}
