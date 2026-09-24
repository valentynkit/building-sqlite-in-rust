use std::fs::File;

use tracing::instrument;

use crate::{
    error::{QueryResult, StorageError, StorageResult},
    helpers::{
        Column, PageType, RecordType, page_header, read_page, read_u32,
        record::{Record, RecordHdr, SerialType},
        to_usize, varint,
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

#[instrument(level = "trace", skip(buf), ret)]
pub fn parse_leaf_cell(buf: &[u8]) -> StorageResult<(TableLeafCell, usize)> {
    let mut off: usize = 0;
    // parsing cell
    let (payload_size, n) = varint(buf)?;
    // offset where cell ends and starts a new one
    off += n;
    let (rowid, n) = varint(&buf[off..])?;
    off += n;

    let record_end_off = off + to_usize(payload_size, "cell payload size")?;

    let (hdr_size, n) = varint(&buf[off..])?;
    // offset where record hdr ends
    let record_hdr_end_off = off + to_usize(hdr_size, "record header size")?;
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
            return Err(StorageError::RecordOverrun);
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
) -> StorageResult<()> {
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Index { .. } = record_type else {
        unreachable!("walk_index called with a {record_type} schema");
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
                let child = (read_u32(&page_buf, ptr)? - 1) as usize;
                walk_index(file, page_size, child, record_type, cells)?;
            }
            let right_child = page_header
                .right_most_child()
                .expect("interior page header always carries a right-most child");
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
) -> StorageResult<()> {
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, page_num)?;
    let page_header = page_header(&page_buf, page_num)?;
    let page_type = page_header.page_type();

    let RecordType::Table { rowid_alias, .. } = record_type else {
        unreachable!("walk called with a {record_type} schema");
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
                let child = (read_u32(&page_buf, ptr)? - 1) as usize;
                walk(file, page_size, child, record_type, keep, cells)?;
            }
            let right_child = page_header
                .right_most_child()
                .expect("interior page header always carries a right-most child");
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
