use crate::{
    error::{QueryResult, StorageError},
    helpers::{ColumnIdx, TableLeafCell},
    sql::ResolvedProjection,
};

pub fn format(rows: &[TableLeafCell], projection: &ResolvedProjection) -> QueryResult<String> {
    let columns = match projection {
        ResolvedProjection::Count => return Ok(rows.len().to_string()),
        ResolvedProjection::Columns(columns) => columns,
    };

    let lines = rows
        .iter()
        .map(|row| {
            let values = &row.record.values;
            let fields = columns
                .iter()
                .map(|&ColumnIdx(i)| {
                    values
                        .get(i)
                        .map(ToString::to_string)
                        .ok_or(StorageError::RecordTooShort {
                            column: i,
                            len: values.len(),
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(fields.join("|"))
        })
        .collect::<QueryResult<Vec<_>>>()?;

    Ok(lines.join("\n"))
}
