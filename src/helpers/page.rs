use crate::{
    constants::DB_HEADER_SIZE,
    error::{StorageError, StorageResult},
    helpers::read_u32,
};

use std::fs::File;
use std::os::unix::fs::FileExt;
use tracing::{debug, instrument};

#[derive(Debug)]
pub struct PageHeader {
    page_type: PageType,
    cell_count: u16,
    cell_pointers: Vec<u16>,
    right_most_child: Option<u32>,
}

#[derive(Debug)]
pub enum PageType {
    InteriorIndex,
    InteriorTable,
    LeafIndex,
    LeafTable,
}

impl TryFrom<u8> for PageType {
    type Error = StorageError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(Self::InteriorIndex),
            0x05 => Ok(Self::InteriorTable),
            0x0a => Ok(Self::LeafIndex),
            0x0d => Ok(Self::LeafTable),
            b => Err(StorageError::InvalidPageType(b)),
        }
    }
}

impl From<PageType> for u8 {
    fn from(value: PageType) -> Self {
        match value {
            PageType::InteriorIndex => 0x02,
            PageType::InteriorTable => 0x05,
            PageType::LeafIndex => 0x0a,
            PageType::LeafTable => 0x0d,
        }
    }
}
impl PageHeader {
    pub const fn new(
        page_type: PageType,
        cell_count: u16,
        cell_pointers: Vec<u16>,
        right_most_child: Option<u32>,
    ) -> Self {
        Self {
            page_type,
            cell_count,
            cell_pointers,
            right_most_child,
        }
    }
    pub const fn cell_count(&self) -> u16 {
        self.cell_count
    }
    pub const fn cell_pointers(&self) -> &Vec<u16> {
        &self.cell_pointers
    }

    pub const fn page_type(&self) -> &PageType {
        &self.page_type
    }

    pub const fn right_most_child(&self) -> Option<u32> {
        self.right_most_child
    }
}

/// Page, HEADER
#[instrument(level = "debug", skip(file, buf, page_size))]
pub fn read_page(
    file: &File,
    buf: &mut [u8],
    page_size: u16,
    page_num: usize,
) -> StorageResult<()> {
    let offset = (page_size as usize * page_num) as u64;
    read_exact_at(file, buf, offset)
}

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> StorageResult<()> {
    file.read_exact_at(buf, offset)
        .map_err(|source| StorageError::Read {
            offset,
            len: buf.len(),
            source,
        })
}

pub fn page_header(page: &[u8], page_num: usize) -> StorageResult<PageHeader> {
    debug!(?page_num, "read page_header");
    // Page 1 carries the 100-byte file header before its b-tree header.
    let hdr_start = if page_num == 0 { DB_HEADER_SIZE } else { 0 };
    let page_type = PageType::try_from(page[hdr_start])?;
    let cell_count = u16::from_be_bytes([page[hdr_start + 3], page[hdr_start + 4]]);

    // Interior headers are 12 bytes: the extra 4 at offset 8 are the right-most child.
    let (len, right_most_child) = match page_type {
        PageType::LeafIndex | PageType::LeafTable => (8, None),
        PageType::InteriorIndex | PageType::InteriorTable => {
            (12, Some(read_u32(page, hdr_start + 8)?))
        }
    };

    let mut offset = hdr_start + len;
    let mut cell_ptrs: Vec<u16> = Vec::with_capacity(cell_count as usize);
    (0..cell_count).for_each(|_| {
        let cell_ptr = u16::from_be_bytes([page[offset], page[offset + 1]]);
        cell_ptrs.push(cell_ptr);
        offset += 2;
    });

    let page_header = PageHeader::new(page_type, cell_count, cell_ptrs, right_most_child);
    debug!(?page_header, "parsed");
    Ok(page_header)
}

pub fn page_size(file: &File) -> StorageResult<u16> {
    let mut db_header = [0u8; DB_HEADER_SIZE];
    read_exact_at(file, &mut db_header, 0)?;
    if !db_header.starts_with(b"SQLite format 3\0") {
        return Err(StorageError::NotADatabase);
    }

    let page_size = u16::from_be_bytes([db_header[16], db_header[17]]);
    debug!(?page_size, "parsed db_header");
    Ok(page_size)
}
