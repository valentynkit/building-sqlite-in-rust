use crate::{
    error::{QueryResult, StorageResult},
    helpers::{Column, ColumnIdx, Database, TableCursor, TableLeafCell, index_rowids, table_seek},
    sql::{Access, Plan},
};

/// Fetches the rows the plan describes. The rowid alias is filled in before the filter
/// runs, so `WHERE id = ...` sees the real value.
pub fn execute(db: &Database, plan: &Plan) -> QueryResult<Vec<TableLeafCell>> {
    let table = plan.table();
    let finish = |mut row: TableLeafCell| {
        if let Some(ColumnIdx(i)) = table.rowid_alias
            && let Some(value) = row.record.values.get_mut(i)
        {
            *value = Column::Int(row.rowid);
        }
        let keep = plan
            .filter()
            .iter()
            .all(|(ColumnIdx(i), expected)| row.record.values.get(*i) == Some(expected));
        keep.then_some(row)
    };

    let rows = match plan.access() {
        Access::Scan => TableCursor::new(db, table.root)?
            .filter_map(|row| row.map(finish).transpose())
            .collect::<StorageResult<Vec<_>>>()?,
        Access::IndexEq { index_root, key } => {
            let mut rows = Vec::new();
            for rowid in index_rowids(db, *index_root, key)? {
                if let Some(row) = table_seek(db, table.root, rowid)?.and_then(finish) {
                    rows.push(row);
                }
            }
            rows
        }
    };
    Ok(rows)
}
