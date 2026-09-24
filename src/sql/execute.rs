use std::{collections::HashMap, fs::File};

use crate::{
    error::{QueryError, QueryResult},
    helpers::{TableLeafCell, walk, walk_index},
    sql::{Ident, Plan, TableSchemas},
};

pub fn btree_walk(
    file: &File,
    plan: Plan,
    table_schemas: TableSchemas,
    page_size: u16,
) -> QueryResult<Vec<TableLeafCell>> {
    let keep = |cell: &TableLeafCell| {
        plan.normal
            .iter()
            .all(|(idx, expected)| cell.record.values.get(*idx) == Some(expected))
    };

    let mut cells: Vec<TableLeafCell> = vec![];

    let mut index_cells: Vec<TableLeafCell> = vec![];
    // TODO: for sicplicity we just handle first index for now, without composite indexes etc...
    if plan.indexed.is_empty() {
        walk(
            file,
            page_size,
            table_schemas.table.rootpage_index()?,
            table_schemas.table.ty(),
            &keep,
            &mut cells,
        )?;
    } else {
        // TODO: we are also not handling that index_schemas may contain indexes that doesn't exist
        // in conditions, ideally we should derive it from indexed_conditions
        let index_schema = table_schemas.indexes[0];

        // traversing the indexes
        walk_index(
            file,
            page_size,
            index_schema.rootpage_index()?,
            index_schema.ty(),
            &mut index_cells,
        )?;

        todo!("use indexes cells to walk through and filter on remaining conditions");
    }

    Ok(cells)
}
