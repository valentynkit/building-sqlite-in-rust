//! Binds names to positions. Everything after this stage works on `ColumnIdx` and
//! `PageIdx`; no `Ident` or name lookup leaves this module.

use std::collections::HashMap;

use tracing::instrument;

use crate::{
    error::{QueryError, QueryResult, StorageError},
    helpers::{Column, ColumnIdx, PageIdx, RecordType, SqliteSchema},
    sql::{Ident, ParsedTokens, Projection},
};

#[derive(Debug)]
pub struct ResolvedQuery {
    pub table: TableInfo,
    pub indexes: Vec<IndexInfo>,
    pub projection: ResolvedProjection,
    pub filter: Vec<(ColumnIdx, Column)>,
}

#[derive(Debug)]
pub struct TableInfo {
    pub root: PageIdx,
    pub rowid_alias: Option<ColumnIdx>,
}

#[derive(Debug)]
pub struct IndexInfo {
    pub column: ColumnIdx,
    pub root: PageIdx,
}

#[derive(Debug)]
pub enum ResolvedProjection {
    Count,
    Columns(Vec<ColumnIdx>),
}

#[instrument(level = "debug", skip(schemas), ret, err)]
pub fn resolve(query: ParsedTokens, schemas: &[SqliteSchema]) -> QueryResult<ResolvedQuery> {
    let TableEntry {
        table,
        indexes,
        columns,
    } = lookup_table(schemas, &query.table)?;

    let column = |name: &Ident| {
        columns
            .get(name)
            .copied()
            .map(ColumnIdx)
            .ok_or_else(|| QueryError::NoSuchColumn(name.clone()))
    };

    let projection = match query.projection {
        Projection::Count => ResolvedProjection::Count,
        Projection::Columns(names) => {
            ResolvedProjection::Columns(names.iter().map(column).collect::<QueryResult<_>>()?)
        }
    };

    let filter = query
        .conditions
        .into_iter()
        .map(|c| Ok((column(&c.column_name)?, c.exp_value)))
        .collect::<QueryResult<_>>()?;

    Ok(ResolvedQuery {
        table,
        indexes,
        projection,
        filter,
    })
}

/// What the catalog knows about one table: where it lives, its indexes, and the
/// name-to-position map for its columns. Only `resolve` reads `columns`.
struct TableEntry<'s> {
    table: TableInfo,
    indexes: Vec<IndexInfo>,
    columns: &'s HashMap<Ident, usize>,
}

/// sqlite_schema row order isn't guaranteed, so the table is found first and each
/// index is then read through its columns.
fn lookup_table<'s>(schemas: &'s [SqliteSchema], name: &Ident) -> QueryResult<TableEntry<'s>> {
    let records: Vec<&SqliteSchema> = schemas.iter().filter(|s| s.tbl_name() == name).collect();

    let mut tables = records
        .iter()
        .filter(|s| matches!(s.ty(), RecordType::Table { .. }));
    let table = tables
        .next()
        .ok_or_else(|| QueryError::NoSuchTable(name.clone()))?;
    if tables.next().is_some() {
        return Err(StorageError::DuplicateTable(name.clone()).into());
    }
    let RecordType::Table {
        parsed_columns,
        rowid_alias,
    } = table.ty()
    else {
        unreachable!("filtered to tables above");
    };

    let indexes = records
        .iter()
        .filter_map(|s| match s.ty() {
            RecordType::Index { col_name } => Some((s, col_name)),
            RecordType::Table { .. } => None,
        })
        .map(|(s, col_name)| {
            let &column = parsed_columns
                .get(col_name)
                .ok_or_else(|| StorageError::UnknownIndexColumn(col_name.clone()))?;
            Ok(IndexInfo {
                column: ColumnIdx(column),
                root: PageIdx::new(s.rootpage_index()?),
            })
        })
        .collect::<QueryResult<_>>()?;

    let table = TableInfo {
        root: PageIdx::new(table.rootpage_index()?),
        rowid_alias: rowid_alias.map(ColumnIdx),
    };
    Ok(TableEntry {
        table,
        indexes,
        columns: parsed_columns,
    })
}
