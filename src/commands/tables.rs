use tracing::{info, instrument};

use crate::helpers::SqliteSchema;

#[instrument(level = "info", skip(schemas), ret)]
pub fn run(schemas: &[SqliteSchema]) -> String {
    info!("executing .tables command");
    let tables = schemas
        .iter()
        .map(|item| item.tbl_name().as_str())
        .collect::<Vec<&str>>()
        .join(" ");
    let out = format!("table names: {tables}");

    info!("finish");
    out
}
