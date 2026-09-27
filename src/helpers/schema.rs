use std::collections::HashMap;

use tracing::{debug, instrument};

use crate::{
    error::{StorageError, StorageResult},
    helpers::{Column, Database, PageIdx, RecordType, TableCursor, int, text, to_usize},
    sql::Ident,
};
// sqlite_schema, CREATE parsing
#[derive(Debug)]
pub struct SqliteSchema {
    ty: RecordType,
    pub tbl_name: Ident,
    rootpage: i64,
    /*
    name: String,
    sql_query: String,
    */
}

impl SqliteSchema {
    pub const fn new(ty: RecordType, tbl_name: Ident, rootpage: i64) -> Self {
        Self {
            ty,
            tbl_name,
            rootpage,
        }
    }

    pub(crate) const fn ty(&self) -> &RecordType {
        &self.ty
    }

    pub(crate) const fn tbl_name(&self) -> &Ident {
        &self.tbl_name
    }

    pub(crate) fn rootpage_index(&self) -> StorageResult<usize> {
        to_usize(self.rootpage - 1, "root page number")
    }

    /*
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn sql_query(&self) -> &str {
        &self.sql_query
    }
    */
}

/// Every row of sqlite_schema. It's an ordinary table b-tree rooted at page 1, so it
/// can span several pages once a database has enough tables and indexes.
#[instrument(level = "debug", skip(db), err)]
pub fn parse_sqlite_schemas(db: &Database) -> StorageResult<Vec<SqliteSchema>> {
    let schemas = TableCursor::new(db, PageIdx::FIRST)?
        .map(|cell| parse(cell?.record.values))
        .collect::<StorageResult<Vec<_>>>()?;
    debug!(count = schemas.len(), "parsed sqlite schemas");
    Ok(schemas)
}

pub fn parse(values: Vec<Column>) -> StorageResult<SqliteSchema> {
    let [ty, _, tbl_name, rootpage, sql]: [Column; 5] = values
        .try_into()
        .map_err(|v: Vec<Column>| StorageError::SchemaRowLen(v.len()))?;

    let ty = text(0, ty)?;
    let tbl_name = Ident::new(text(2, tbl_name)?);
    let sql = text(4, sql)?.to_ascii_lowercase();

    if !sql.starts_with(&format!("create {ty}")) {
        return Err(StorageError::UnsupportedSchemaSql {
            reason: "does not start with CREATE <type>",
            sql,
        });
    }

    let ty = match ty.as_str() {
        "table" => parse_table_sql(&sql)?,
        "index" => parse_index_sql(&sql)?,
        _ => return Err(StorageError::UnknownObjectType(ty)),
    };

    let schema = SqliteSchema::new(ty, tbl_name, int(3, rootpage)?);
    debug!(?schema, "parsed");
    Ok(schema)
}

fn parse_index_sql(sql: &str) -> StorageResult<RecordType> {
    Ok(RecordType::Index {
        col_name: Ident::new(between_parens(sql)?.trim()),
    })
}

fn parse_table_sql(sql: &str) -> StorageResult<RecordType> {
    let mut parsed_columns: HashMap<Ident, usize> = HashMap::new();
    let mut rowid_alias = None;

    for (idx, definition) in between_parens(sql)?.split(',').map(str::trim).enumerate() {
        let name = definition.split_whitespace().next().ok_or_else(|| {
            StorageError::UnsupportedSchemaSql {
                reason: "empty column definition",
                sql: sql.to_owned(),
            }
        })?;
        parsed_columns.insert(Ident::new(name), idx);
        if definition.contains("integer primary key") {
            rowid_alias = Some(idx);
        }
    }

    debug!(?parsed_columns, ?rowid_alias);
    Ok(RecordType::Table {
        parsed_columns,
        rowid_alias,
    })
}

/// Text between the first `(` and the last `)` of a CREATE statement.
fn between_parens(sql: &str) -> StorageResult<&str> {
    let unsupported = |reason| StorageError::UnsupportedSchemaSql {
        reason,
        sql: sql.to_owned(),
    };
    let open = sql.find('(').ok_or_else(|| unsupported("missing `(`"))?;
    let close = sql.rfind(')').ok_or_else(|| unsupported("missing `)`"))?;
    if close < open {
        return Err(unsupported("`)` before `(`"));
    }
    Ok(&sql[open + 1..close])
}
