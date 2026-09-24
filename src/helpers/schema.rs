use tracing::{debug, instrument};

use crate::{
    error::StorageResult,
    helpers::{RecordType, page_header, parse_leaf_cell, to_usize},
    sql::{Ident, parse},
};
// sqlite_schema, CREATE parsing
#[derive(Debug)]
pub struct SqliteSchema {
    ty: RecordType,
    tbl_name: Ident,
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

#[instrument(level = "debug", skip(page_buf), err)]
pub fn parse_sqlite_schemas(page_buf: &[u8]) -> StorageResult<Vec<SqliteSchema>> {
    let page_header = page_header(page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
        schemas.push(parse(cell.record.values)?);
    }
    debug!(count = schemas.len(), "parsed sqlite schemas");

    Ok(schemas)
}
