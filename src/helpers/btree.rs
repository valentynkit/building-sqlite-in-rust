//! Reading SQLite's two kinds of b-tree. Table trees keep rows only in leaves and key
//! interior cells by rowid. Index trees keep entries in interior cells too, so an
//! in-order walk must visit child `i`, then cell `i`, then the next child.

use std::cmp::Ordering;

use crate::{
    error::{StorageError, StorageResult},
    helpers::{Column, Database, Page, PageIdx, PageType, Record, to_usize, varint},
};

/// A table b-tree leaf cell: payload size, rowid, then the record.
/// Overflow pages are out of scope, so the payload must fit on the page.
#[derive(Debug)]
pub struct TableLeafCell {
    pub rowid: i64,
    pub record: Record,
}

/// One page on the path from the root, and the next cell or child to visit in it.
struct Frame {
    page: Page,
    next: u16,
}

/// Every row of a table b-tree, in rowid order, one page per tree level in memory.
pub struct TableCursor<'db> {
    db: &'db Database,
    stack: Vec<Frame>,
}

impl<'db> TableCursor<'db> {
    pub fn new(db: &'db Database, root: PageIdx) -> StorageResult<Self> {
        let root = db.page(root)?;
        Ok(Self {
            db,
            stack: vec![Frame {
                page: root,
                next: 0,
            }],
        })
    }
}

impl Iterator for TableCursor<'_> {
    type Item = StorageResult<TableLeafCell>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = loop {
            let top = self.stack.last_mut()?;
            let cell_count = top.page.header().cell_count();

            match top.page.header().page_type() {
                PageType::LeafTable => {
                    if top.next == cell_count {
                        self.stack.pop();
                        continue;
                    }
                    let i = top.next;
                    top.next += 1;
                    break top.page.cell(i).and_then(table_leaf_cell);
                }
                PageType::InteriorTable => {
                    if top.next > cell_count {
                        self.stack.pop();
                        continue;
                    }
                    let child = top.page.child(top.next);
                    top.next += 1;
                    match child.and_then(|idx| self.db.page(idx)) {
                        Ok(page) => self.stack.push(Frame { page, next: 0 }),
                        Err(e) => break Err(e),
                    }
                }
                found => break Err(wrong_page(&top.page, "table", found)),
            }
        };
        if item.is_err() {
            self.stack.clear(); // a failed read ends the scan instead of repeating
        }
        Some(item)
    }
}

/// Rowids of every index entry whose key equals `key`, in index order. Descends only
/// into children that can hold `key`, so the cost grows with matches, not table size.
pub fn index_rowids(db: &Database, root: PageIdx, key: &Column) -> StorageResult<Vec<i64>> {
    let mut rowids = Vec::new();
    search_index(db, root, key, &mut rowids)?;
    Ok(rowids)
}

fn search_index(
    db: &Database,
    idx: PageIdx,
    key: &Column,
    rowids: &mut Vec<i64>,
) -> StorageResult<()> {
    let page = db.page(idx)?;
    let interior = match page.header().page_type() {
        PageType::InteriorIndex => true,
        PageType::LeafIndex => false,
        found => return Err(wrong_page(&page, "index", found)),
    };

    let cell_count = page.header().cell_count();
    for i in 0..cell_count {
        let (entry_key, rowid) = index_entry(page.cell(i)?, interior)?;
        let order = key.sqlite_cmp(&entry_key);
        // Child `i` holds keys <= entry_key, so it can contain `key` unless key is larger.
        if interior && order != Ordering::Greater {
            search_index(db, page.child(i)?, key, rowids)?;
        }
        match order {
            Ordering::Equal => rowids.push(rowid),
            Ordering::Less => return Ok(()), // every later entry is larger still
            Ordering::Greater => {}
        }
    }
    if interior {
        search_index(db, page.child(cell_count)?, key, rowids)?;
    }
    Ok(())
}

/// The row with `rowid`, following one path from the root: at each interior page, the
/// first cell whose key is >= rowid, or the right-most child.
pub fn table_seek(
    db: &Database,
    root: PageIdx,
    rowid: i64,
) -> StorageResult<Option<TableLeafCell>> {
    let mut page = db.page(root)?;
    loop {
        let cell_count = page.header().cell_count();
        match page.header().page_type() {
            PageType::InteriorTable => {
                let mut child = cell_count;
                for i in 0..cell_count {
                    if rowid <= table_interior_key(page.cell(i)?)? {
                        child = i;
                        break;
                    }
                }
                page = db.page(page.child(child)?)?;
            }
            PageType::LeafTable => {
                for i in 0..cell_count {
                    let cell = page.cell(i)?;
                    if table_leaf_rowid(cell)? == rowid {
                        return table_leaf_cell(cell).map(Some);
                    }
                }
                return Ok(None);
            }
            found => return Err(wrong_page(&page, "table", found)),
        }
    }
}

/// Leaf table cell: payload-size varint, rowid varint, record.
fn table_leaf_cell(cell: &[u8]) -> StorageResult<TableLeafCell> {
    let (payload_size, a) = varint(cell)?;
    let (rowid, b) = varint(&cell[a..])?;
    let record = Record::parse(payload(cell, a + b, payload_size)?)?;
    Ok(TableLeafCell { rowid, record })
}

fn table_leaf_rowid(cell: &[u8]) -> StorageResult<i64> {
    let (_, n) = varint(cell)?;
    Ok(varint(&cell[n..])?.0)
}

/// Interior table cell: 4-byte left child, then the largest rowid in that child.
fn table_interior_key(cell: &[u8]) -> StorageResult<i64> {
    Ok(varint(cell.get(4..).ok_or(StorageError::TruncatedVarint)?)?.0)
}

/// Index cell: on interior pages a 4-byte left child first, then payload-size varint
/// and a record of `[indexed column, rowid]`.
fn index_entry(cell: &[u8], interior: bool) -> StorageResult<(Column, i64)> {
    let start = if interior { 4 } else { 0 };
    let (payload_size, n) = varint(cell.get(start..).ok_or(StorageError::TruncatedVarint)?)?;
    let mut values = Record::parse(payload(cell, start + n, payload_size)?)?.values;

    let found = values.len();
    let (Some(Column::Int(rowid)), Some(key)) = (values.pop(), values.into_iter().next()) else {
        return Err(StorageError::MalformedIndexEntry(found));
    };
    Ok((key, rowid))
}

fn payload(cell: &[u8], start: usize, size: i64) -> StorageResult<&[u8]> {
    let end = start + to_usize(size, "cell payload size")?;
    cell.get(start..end).ok_or(StorageError::Truncated {
        need: end,
        have: cell.len(),
    })
}

fn wrong_page(page: &Page, tree: &'static str, found: PageType) -> StorageError {
    StorageError::WrongPageType {
        page: page.idx().value() + 1,
        tree,
        found: found.into(),
    }
}
