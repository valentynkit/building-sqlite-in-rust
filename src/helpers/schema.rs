use crate::{
    error::StorageResult,
    helpers::{RecordType, to_usize},
    sql::Ident,
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
