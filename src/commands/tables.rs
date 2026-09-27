use crate::helpers::{RecordType, SqliteSchema};

/// User tables in name order, as the sqlite shell prints them. Internal `sqlite_`
/// tables and indexes are left out.
pub fn run(schemas: &[SqliteSchema]) -> String {
    let mut names: Vec<&str> = schemas
        .iter()
        .filter(|s| matches!(s.ty(), RecordType::Table { .. }))
        .map(|s| s.tbl_name().as_str())
        .filter(|name| !name.starts_with("sqlite_"))
        .collect();
    names.sort_unstable();
    names.join(" ")
}
