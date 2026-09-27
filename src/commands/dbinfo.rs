use crate::helpers::{Database, SqliteSchema};

/// `number of tables` counts sqlite_schema rows, which is what the sqlite shell's
/// `.dbinfo` reports as "number of tables" plus indexes and views.
pub fn run(db: &Database, schemas: &[SqliteSchema]) -> String {
    format!(
        "database page size: {}\nnumber of tables: {}",
        db.page_size(),
        schemas.len()
    )
}
