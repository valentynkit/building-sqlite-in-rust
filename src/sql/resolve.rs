use crate::{
    error::QueryError,
    helpers::{QueryResult, RecordType, SqliteSchema},
    sql::Ident,
};

pub fn parse_table_and_index_schemas<'a>(
    schemas: &'a [SqliteSchema],
    tbl_name: Ident,
) -> QueryResult<(Option<&'a SqliteSchema>, Vec<&'a SqliteSchema>)> {
    // we may have several schema for the same tbl_name, for example:
    // 1 for table, and 3 for indexes for this table.
    let records: Vec<&SqliteSchema> = schemas
        .iter()
        .filter(|s| s.tbl_name().eq_ignore_ascii_case(tbl_name))
        .collect();

    if records.is_empty() {
        return Err(QueryError::NoSuchTable((tbl_name).to_owned()));
    }
    let mut table_schema: Option<&SqliteSchema> = None;
    let mut index_schemas: Vec<&SqliteSchema> = vec![];
    for record in records {
        match record.ty() {
            RecordType::Table {
                parsed_columns: _,
                rowid_alias: _,
            } => {
                if let Some(table_schema) = table_schema {
                    return Err(QueryError::DuplicatedTable(
                        table_schema.tbl_name().to_owned(),
                    ));
                }
                table_schema = Some(record);
            }
            RecordType::Index { col_name: _ } => {
                index_schemas.push(record);
            }
        }
    }
    Ok((table_schema, index_schemas))
}
