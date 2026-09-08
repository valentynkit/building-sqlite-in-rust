use tracing::debug;

use crate::constants::{DB_HEADER_SIZE, PAGE_SIZE};
use std::fs::File;
use std::os::unix::fs::FileExt;

pub(crate) struct DbHeader {
    page_size: u16,
}

impl DbHeader {
    pub(crate) fn new(page_size: u16) -> DbHeader {
        DbHeader { page_size }
    }
    pub(crate) fn page_size(&self) -> u16 {
        self.page_size
    }
}

#[derive(Debug)]
pub(crate) struct PageHeader {
    page_type: PageType,
    cell_count: u16,
    cell_pointers: Vec<u16>,
}

impl PageHeader {
    pub(crate) fn new(page_type: PageType, cell_count: u16, cell_ptrs: Vec<u16>) -> PageHeader {
        PageHeader {
            page_type,
            cell_count,
            cell_pointers: cell_ptrs,
        }
    }
    pub(crate) fn cell_count(&self) -> u16 {
        self.cell_count
    }
    pub(crate) fn cell_pointers(&self) -> &Vec<u16> {
        &self.cell_pointers
    }
}
#[derive(Debug)]
pub(crate) enum PageType {
    InteriorIndex,
    InteriorTable,
    LeafIndex,
    LeafTable,
}

impl TryFrom<u8> for PageType {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(PageType::InteriorIndex),
            0x05 => Ok(PageType::InteriorTable),
            0x0a => Ok(PageType::LeafIndex),
            0x0d => Ok(PageType::LeafTable),
            b => anyhow::bail!("invalid page type: {b:#x}"),
        }
    }
}
pub(crate) fn db_header(file: &File) -> anyhow::Result<DbHeader> {
    let mut db_header: [u8; 100] = [0; 100];
    file.read_exact_at(&mut db_header, 0)?;

    let page_size = u16::from_be_bytes([db_header[16], db_header[17]]);
    debug!(?page_size, "parsed db_header");
    Ok(DbHeader::new(page_size))
}

pub(crate) fn page_header(page: Vec<u8>, num_page: usize) -> anyhow::Result<PageHeader> {
    debug!(?num_page, "read page_header");
    let mut offset: usize = 0;
    let mut out: [u8; 12] = [0; 12];
    if num_page == 0 {
        offset += DB_HEADER_SIZE;
    }
    let page_type = PageType::try_from(page[offset])?;

    let len: usize = match page_type {
        PageType::LeafIndex | PageType::LeafTable => 8,
        PageType::InteriorIndex | PageType::InteriorTable => 12,
    };

    let cell_count = u16::from_be_bytes([page[offset + 3], page[offset + 4]]);
    let mut cell_ptrs: Vec<u16> = Vec::with_capacity(cell_count as usize);

    offset += len;
    for cell_n in 0..cell_count {
        let cell_ptr = u16::from_be_bytes([page[offset], page[offset + 1]]);
        debug!(?offset, ?cell_n, ?cell_ptr, "parsing cell pointers");
        cell_ptrs.push(cell_ptr);
        offset += 2;
    }

    let page_header = PageHeader::new(page_type, cell_count, cell_ptrs);
    debug!(?page_header, "parsed");
    Ok(page_header)
}

pub(crate) fn parse_cell(buf: &mut [u8], mut off: usize) -> anyhow::Result<()> {
    let (cell_size, n) = varint(&buf[off..])?;
    off += n;
    let (row_id, n) = varint(&buf[off..])?;
    off += n;
    let (rec_size, n) = varint(&buf[off..])?;
    off += n;

    while off < (rec_size + off) - n {}
    let (rec_size, n) = varint(&buf[off..])?;
    /*
        TableLeafCell {
            cell_size,
            rowid
        }
    */
    todo!()
}
pub(crate) fn read_page(
    file: &File,
    buf: &mut [u8],
    page_size: u16,
    page_num: usize,
) -> anyhow::Result<()> {
    let offset = page_size as usize * page_num;
    debug!(?offset, ?page_size, ?page_num, "loading the page");
    file.read_exact_at(buf, offset as u64)?;
    Ok(())
}

pub(crate) fn varint(buf: &[u8]) -> anyhow::Result<(i64, usize)> {
    const MORE_FOLLOWS: u8 = 0b1000_0000; // top bit
    const PAYLOAD: u8 = 0b0111_1111; // low 7 bits

    let mut value: u64 = 0;
    let mut i = 0;
    while i < 8 {
        let byte = *buf
            .get(i)
            .ok_or_else(|| anyhow::anyhow!("truncated variant"))?;
        let payload = (byte & PAYLOAD) as u64;
        value = (value << 7) | payload; // make room for 7 bits, append them
        i += 1;
        if byte & MORE_FOLLOWS == 0 {
            return Ok((value as i64, i)); // top bit clear: this was the last byte
        }
    }

    let byte = *buf
        .get(8)
        .ok_or_else(|| anyhow::anyhow!("truncated varint"))?;
    value = (value << 8) | byte as u64;
    Ok((value as i64, 9))
}

/// Record-format serial type. Spec: https://www.sqlite.org/fileformat.html#record_format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SerialType {
    Null,
    I8,
    I16,
    I24,
    I32,
    I48,
    I64,
    F64,
    /// Codes 8 and 9: literal 0 / 1, no bytes in the body.
    Zero,
    One,
    /// Code N >= 12, even. Payload is (N-12)/2 bytes.
    Blob(usize),
    /// Code N >= 13, odd. Payload is (N-13)/2 bytes.
    Text(usize),
}

impl SerialType {
    /// Byte width of this column in the record body.
    pub(crate) fn size(self) -> usize {
        match self {
            Self::Null | Self::Zero | Self::One => 0,
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I24 => 3,
            Self::I32 => 4,
            Self::I48 => 6,
            Self::I64 | Self::F64 => 8,
            Self::Blob(n) | Self::Text(n) => n,
        }
    }
}

impl TryFrom<u64> for SerialType {
    type Error = anyhow::Error;

    fn try_from(code: u64) -> Result<Self, Self::Error> {
        Ok(match code {
            0 => Self::Null,
            1 => Self::I8,
            2 => Self::I16,
            3 => Self::I24,
            4 => Self::I32,
            5 => Self::I48,
            6 => Self::I64,
            7 => Self::F64,
            8 => Self::Zero,
            9 => Self::One,
            10 | 11 => anyhow::bail!("reserved serial type: {code}"),
            n if n % 2 == 0 => Self::Blob(((n - 12) / 2) as usize),
            n => Self::Text(((n - 13) / 2) as usize),
        })
    }
}

/// One decoded column. All integer widths collapse to i64, matching sqlite's own model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Int(i64),
    Float(f64),
    Blob(Vec<u8>),
    Text(String),
}

/// One row: the record header's serial types applied to the body, in column order.
#[derive(Debug)]
pub(crate) struct RecordHdr {
    hdr_size: usize,
    serial_types: Vec<SerialType>,
}

#[derive(Debug)]
pub(crate) struct Record {
    pub(crate) values: Vec<Value>,
}

/// Table b-tree leaf cell: payload size varint, rowid varint, then the record.
/// Overflow pages ignored on purpose, out of scope for this challenge.
#[derive(Debug)]
pub(crate) struct TableLeafCell {
    pub(crate) cell_size: i64,
    pub(crate) rowid: i64,
    pub(crate) record: Record,
}

#[cfg(test)]
mod tests {
    use super::SerialType;

    #[test]
    fn varint_spec_examples() {
        assert_eq!(varint(&[0x2a]).unwrap(), (42, 1));
        assert_eq!(varint(&[0xb3, 0x33]).unwrap(), (6579, 2));
        assert_eq!(varint(&[0x81, 0xbf, 0x81, 0x3f]).unwrap(), (3129535, 4));
        // trailing bytes ignored, only consumed count matters
        assert_eq!(varint(&[0x78, 0x03, 0x07]).unwrap(), (120, 1));
        assert!(varint(&[0x80]).is_err());
    }

    #[test]
    fn serial_type_matches_spec_example() {
        // From the sample.db "oranges" cell: 17 1b 1b 01 81 47
        assert_eq!(SerialType::try_from(23).unwrap(), SerialType::Text(5));
        assert_eq!(SerialType::try_from(27).unwrap(), SerialType::Text(7));
        assert_eq!(SerialType::try_from(1).unwrap().size(), 1);
        assert_eq!(SerialType::try_from(199).unwrap(), SerialType::Text(93));
        assert_eq!(SerialType::try_from(12).unwrap(), SerialType::Blob(0));
        assert!(SerialType::try_from(10).is_err());
    }
}
