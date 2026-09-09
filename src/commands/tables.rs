use tracing::info;

use crate::commands::helpers::SqliteSchema;

pub(crate) fn run(schemas: Vec<SqliteSchema>) -> anyhow::Result<String> {
    info!("executing .tables command");
    let tables = schemas
        .iter()
        .map(|schema| schema.tbl_name())
        .collect::<Vec<&str>>()
        .join(" ");
    let out = format!("table names: {tables}");

    info!("finish");
    Ok(out)
}
