use super::Result;
use crate::helpers::RecordType;
// sqlite_schema, CREATE parsing
#[derive(Debug)]
pub struct SqliteSchema {
    ty: RecordType,
    tbl_name: String,
    rootpage: i64,
    /*
    name: String,
    sql_query: String,
    */
}

impl SqliteSchema {
    pub fn new(ty: RecordType, tbl_name: String, rootpage: i64) -> Self {
        Self {
            ty,
            tbl_name,
            rootpage,
        }
    }

    pub(crate) fn ty(&self) -> &RecordType {
        &self.ty
    }

    pub(crate) fn tbl_name(&self) -> &str {
        &self.tbl_name
    }

    pub(crate) fn rootpage_index(&self) -> Result<usize> {
        Ok(usize::try_from(self.rootpage - 1)?)
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
