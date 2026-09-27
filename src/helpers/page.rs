use crate::{
    constants::DB_HEADER_SIZE,
    error::{StorageError, StorageResult},
};

use std::fs::File;
use std::os::unix::fs::FileExt;
use tracing::debug;

/// 0-based page position in the file: sqlite page number minus 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageIdx(usize);

impl PageIdx {
    /// Page 1, which holds the file header and sqlite_schema.
    pub const FIRST: Self = Self(0);

    pub fn new(value: usize) -> Self {
        Self(value)
    }

    /// From a 1-based page number as stored in the file. 0 never names a page.
    pub fn from_page_number(number: u32) -> StorageResult<Self> {
        number
            .checked_sub(1)
            .map(|idx| Self(idx as usize))
            .ok_or(StorageError::OutOfRange {
                what: "page number",
                value: 0,
            })
    }

    pub fn value(&self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColumnIdx(pub usize);

pub struct Database {
    file: File,
    page_size: usize,
}

impl Database {
    pub fn new(file: File, page_size: usize) -> Self {
        Self { file, page_size }
    }

    pub fn open(path: &str) -> StorageResult<Self> {
        let file = File::open(path).map_err(|source| StorageError::Open {
            path: path.to_string(),
            source,
        })?;

        let mut db_header = [0u8; DB_HEADER_SIZE];
        read_exact_at(&file, &mut db_header, 0)?;
        if !db_header.starts_with(b"SQLite format 3\0") {
            return Err(StorageError::NotADatabase);
        }

        let page_size = usize::from(u16::from_be_bytes([db_header[16], db_header[17]]));

        Ok(Database::new(file, page_size))
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    pub fn page(&self, idx: PageIdx) -> StorageResult<Page> {
        let mut bytes = vec![0u8; self.page_size];
        read_exact_at(&self.file, &mut bytes, (idx.0 * self.page_size) as u64)?;
        Page::new(idx, bytes)
    }
}

pub struct Page {
    idx: PageIdx,
    bytes: Vec<u8>,
    header: PageHeader,
}

impl Page {
    fn new(idx: PageIdx, bytes: Vec<u8>) -> StorageResult<Self> {
        let header = parse_header(&bytes, idx)?;
        debug!(?idx, ?header, "loaded page");
        Ok(Self { idx, bytes, header })
    }

    pub const fn idx(&self) -> PageIdx {
        self.idx
    }

    pub const fn header(&self) -> &PageHeader {
        &self.header
    }

    /// Bytes of cell `i`, from its pointer to the end of the page. Decoders read only
    /// what the cell's own lengths say.
    pub fn cell(&self, i: u16) -> StorageResult<&[u8]> {
        let &ptr =
            self.header
                .cell_pointers
                .get(usize::from(i))
                .ok_or(StorageError::Truncated {
                    need: usize::from(i) + 1,
                    have: self.header.cell_pointers.len(),
                })?;
        self.bytes
            .get(usize::from(ptr)..)
            .ok_or(StorageError::Truncated {
                need: usize::from(ptr),
                have: self.bytes.len(),
            })
    }

    /// Child `i` of an interior page, in key order. Children `0..cell_count` are the
    /// 4-byte left pointers at the start of each cell; child `cell_count` is the
    /// right-most pointer from the header.
    pub fn child(&self, i: u16) -> StorageResult<PageIdx> {
        debug_assert!(
            i <= self.header.cell_count,
            "child {i} past the right-most child"
        );
        let number = if i < self.header.cell_count {
            read_u32(self.cell(i)?, 0)?
        } else {
            self.header
                .right_most_child
                .expect("interior page header always carries a right-most child")
        };
        PageIdx::from_page_number(number)
    }
}

fn parse_header(bytes: &[u8], idx: PageIdx) -> StorageResult<PageHeader> {
    // Page 1 carries the 100-byte file header before its b-tree header.
    let start = if idx == PageIdx::FIRST {
        DB_HEADER_SIZE
    } else {
        0
    };
    let byte = |at: usize| {
        bytes.get(at).copied().ok_or(StorageError::Truncated {
            need: at + 1,
            have: bytes.len(),
        })
    };

    let page_type = PageType::try_from(byte(start)?)?;
    let cell_count = u16::from_be_bytes([byte(start + 3)?, byte(start + 4)?]);

    // Interior headers are 12 bytes: the extra 4 at offset 8 are the right-most child.
    let (len, right_most_child) = match page_type {
        PageType::LeafIndex | PageType::LeafTable => (8, None),
        PageType::InteriorIndex | PageType::InteriorTable => {
            (12, Some(read_u32(bytes, start + 8)?))
        }
    };

    let array_start = start + len;
    let array_end = array_start + 2 * usize::from(cell_count);
    let cell_pointers = bytes
        .get(array_start..array_end)
        .ok_or(StorageError::Truncated {
            need: array_end,
            have: bytes.len(),
        })?
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();

    Ok(PageHeader::new(
        page_type,
        cell_count,
        cell_pointers,
        right_most_child,
    ))
}

/// Big-endian `u32` at `at`, the width of every page number in the format.
fn read_u32(buf: &[u8], at: usize) -> StorageResult<u32> {
    buf.get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or(StorageError::Truncated {
            need: at + 4,
            have: buf.len(),
        })
}

#[derive(Debug)]
pub struct PageHeader {
    page_type: PageType,
    cell_count: u16,
    cell_pointers: Vec<u16>,
    right_most_child: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub const fn page_type(&self) -> PageType {
        self.page_type
    }
}

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> StorageResult<()> {
    file.read_exact_at(buf, offset)
        .map_err(|source| StorageError::Read {
            offset,
            len: buf.len(),
            source,
        })
}
