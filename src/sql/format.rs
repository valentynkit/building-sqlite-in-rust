use std::collections::HashMap;

use crate::{
    error::{QueryError, QueryResult, StorageError},
    helpers::TableLeafCell,
    sql::Ident,
};

pub fn filter_what_col(
    cells: &[TableLeafCell],
    what: &[Ident],
    parsed_columns: &HashMap<Ident, usize>,
) -> QueryResult<String> {
    let mut out: Vec<String> = vec![String::new(); cells.len() * what.len()];
    for (idx_col, col) in what.iter().enumerate() {
        let Some(&t_idx) = parsed_columns.get(col) else {
            return Err(QueryError::NoSuchColumn(col.clone()));
        };

        for (idx_row, row) in cells.iter().enumerate() {
            let value = row
                .record
                .values
                .get(t_idx)
                .ok_or(StorageError::RecordTooShort {
                    column: t_idx,
                    len: row.record.values.len(),
                })?;

            let idx = idx_col + (idx_row * what.len());
            out[idx] = value.to_string();
        }
    }

    Ok(out
        .chunks(what.len())
        .map(|row| row.join("|"))
        .collect::<Vec<String>>()
        .join("\n"))
}
