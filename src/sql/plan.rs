use tracing::instrument;

use crate::{
    helpers::{Column, ColumnIdx, PageIdx},
    sql::{ResolvedProjection, ResolvedQuery, TableInfo},
};

pub enum Access {
    Scan,
    IndexEq { index_root: PageIdx, key: Column },
}

pub struct Plan {
    access: Access,
    table: TableInfo,
    filter: Vec<(ColumnIdx, Column)>,
    projection: ResolvedProjection,
}

impl Plan {
    pub fn new(
        access: Access,
        table: TableInfo,
        filter: Vec<(ColumnIdx, Column)>,
        projection: ResolvedProjection,
    ) -> Self {
        Self {
            access,
            table,
            filter,
            projection,
        }
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    pub fn table(&self) -> &TableInfo {
        &self.table
    }

    pub fn filter(&self) -> &[(ColumnIdx, Column)] {
        &self.filter
    }

    pub fn projection(&self) -> &ResolvedProjection {
        &self.projection
    }
}

/// Chooses how to fetch rows. The returned plan carries everything execute needs, so
/// the resolved query is consumed here.
#[instrument(level = "debug", skip(resolved_query))]
pub fn plan(resolved_query: ResolvedQuery) -> Plan {
    let indexed = resolved_query
        .filter
        .iter()
        .enumerate()
        .find_map(|(i, (col, value))| {
            let index = resolved_query.indexes.iter().find(|ix| ix.column == *col)?;
            Some((i, index.root, value.clone()))
        });
    let mut filter = resolved_query.filter.clone();
    let access = match indexed {
        Some((i, index_root, key)) => {
            filter.remove(i);
            Access::IndexEq { index_root, key }
        }
        None => Access::Scan,
    };
    let table = resolved_query.table;
    Plan::new(access, table, filter, resolved_query.projection)
}
