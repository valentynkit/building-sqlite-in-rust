use std::fs::File;

use super::Result;
use tracing::{error, instrument};

use crate::{
    error::{FormatError, QueryError},
    helpers::{
        Column, PageType, QueryResult, RecordType, SqliteSchema, page_header, read_page,
        record::{Record, RecordHdr, SerialType},
        varint,
    },
    sql::{Plan, TableSchemas},
};

// walk
/// Table b-tree leaf cell: payload size varint, rowid varint, then the record.
/// Overflow pages ignored on purpose, out of scope for this challenge.
///
#[derive(Debug)]
pub struct TableLeafCell {
    pub rowid: i64,
    pub record: Record,
}

#[instrument(level = "debug", skip(buf), ret, err)]
pub fn parse_leaf_cell(buf: &[u8]) -> Result<(TableLeafCell, usize)> {
    let mut off: usize = 0;
    // parsing cell
    let (payload_size, n) = varint(buf)?;
    // offset where cell ends and starts a new one
    off += n;
    let (rowid, n) = varint(&buf[off..])?;
    off += n;

    let record_end_off = off + usize::try_from(payload_size)?;

    let (hdr_size, n) = varint(&buf[off..])?;
    // offset where record hdr ends
    let record_hdr_end_off = off + usize::try_from(hdr_size)?;
    off += n;
    let mut serial_types: Vec<SerialType> = vec![];

    // parsing record header
    while off < record_hdr_end_off {
        let (s_type, n) = varint(&buf[off..])?;
        let serial_type = SerialType::try_from(s_type)?;
        serial_types.push(serial_type);
        off += n;
    }

    let rec_hdr = RecordHdr::new(serial_types);

    let mut values: Vec<Column> = Vec::with_capacity(rec_hdr.serial_types().len());
    for &s_type in rec_hdr.serial_types() {
        // Zero-width types (NULL, 0, 1) may sit exactly at the end of the record.
        if off + s_type.size() > record_end_off {
            return Err(FormatError::RecordOverrun);
        }
        let (col, n) = Column::parse(&buf[off..], s_type)?;
        values.push(col);
        off += n;
    }

    let record = Record { values };
    let cell_parsed = TableLeafCell { rowid, record };
    Ok((cell_parsed, off))
}

pub fn walk_index(
    file: &File,
    page_size: u16,
    page_num: usize,
    record_type: &RecordType,
    cells: &mut Vec<TableLeafCell>,
) -> QueryResult<()> {
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Index { col_name: _ } = record_type else {
        error!("expected to have RecordType::Index");
        return Err(QueryError::WrongRecordType {
            expected: "Index".to_owned(),
            actual: record_type.to_string(),
        });
    };
    match page_type {
        PageType::LeafIndex => {
            for &ptr in page_header.cell_pointers() {
                let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
                cells.push(cell);
            }
        }
        PageType::InteriorIndex => {
            for &ptr in page_header.cell_pointers() {
                let ptr = ptr as usize;
                let child = (u32::from_be_bytes(page_buf[ptr..ptr + 4].try_into()?) - 1) as usize;
                walk_index(file, page_size, child, record_type, cells)?;
            }
            let Some(right_child) = page_header.right_most_child() else {
                error!("no right child found for InteriorIndex");
                return Err(FormatError::PageType(PageType::InteriorIndex.into()).into());
            };
            walk_index(
                file,
                page_size,
                (right_child - 1) as usize,
                record_type,
                cells,
            )?;
        }
        PageType::LeafTable | PageType::InteriorTable => {
            unimplemented!("index traversing unimplemented!")
        }
    }
    Ok(())
}

pub fn walk(
    file: &File,
    page_size: u16,
    page_num: usize,
    record_type: &RecordType,
    keep: &dyn Fn(&TableLeafCell) -> bool,
    cells: &mut Vec<TableLeafCell>,
) -> QueryResult<()> {
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Table {
        parsed_columns: _,
        rowid_alias,
    } = record_type
    else {
        error!("expected to have RecordType::Table");
        return Err(QueryError::WrongRecordType {
            expected: "Table".to_owned(),
            actual: record_type.to_string(),
        });
    };

    match page_type {
        PageType::LeafTable => {
            for &ptr in page_header.cell_pointers() {
                let (mut cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
                if let Some(v) = rowid_alias.and_then(|i| cell.record.values.get_mut(i)) {
                    *v = Column::Int(cell.rowid);
                }
                if keep(&cell) {
                    cells.push(cell);
                }
            }
        }
        PageType::InteriorTable => {
            for &ptr in page_header.cell_pointers() {
                let ptr = ptr as usize;
                let child = (u32::from_be_bytes(page_buf[ptr..ptr + 4].try_into()?) - 1) as usize;
                walk(file, page_size, child, record_type, keep, cells)?;
            }
            let Some(right_child) = page_header.right_most_child() else {
                return Err(FormatError::PageType(PageType::InteriorTable.into()).into());
            };
            walk(
                file,
                page_size,
                (right_child - 1) as usize,
                record_type,
                keep,
                cells,
            )?;
        }
        PageType::LeafIndex | PageType::InteriorIndex => {
            unimplemented!("index traversing unimplemented!")
        }
    }
    Ok(())
}

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
