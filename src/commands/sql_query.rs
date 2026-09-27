use tracing::{info, instrument};

use crate::{
    error::QueryResult,
    helpers::{Database, SqliteSchema},
    sql::{execute, format, parse_query, plan, resolve, tokenize},
};

#[instrument(level = "info", skip(schemas, database), err)]
pub fn run(database: &Database, schemas: &[SqliteSchema], query: &[String]) -> QueryResult<String> {
    let query = query.join(" ");
    info!(%query, "executing sql");

    let tokens = tokenize(&query)?;
    let parsed = parse_query(tokens)?;
    let resolved = resolve(parsed, schemas)?;
    let plan = plan(resolved);
    let rows = execute(database, &plan)?;
    format(&rows, plan.projection())
}
