use tracing::{debug, info};

use crate::constants::DB_HEADER_SIZE;
use crate::error::FormatError;
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
pub(crate) enum RecordType {
    Table,
    Index,
}

#[derive(Debug)]
pub(crate) enum PageType {
    InteriorIndex,
    InteriorTable,
    LeafIndex,
    LeafTable,
}

impl TryFrom<u8> for PageType {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(PageType::InteriorIndex),
            0x05 => Ok(PageType::InteriorTable),
            0x0a => Ok(PageType::LeafIndex),
            0x0d => Ok(PageType::LeafTable),
            b => Err(FormatError::PageType(b)),
        }
    }
}
pub(crate) fn db_header(file: &File) -> Result<DbHeader, FormatError> {
    let mut db_header: [u8; 100] = [0; 100];
    file.read_exact_at(&mut db_header, 0)?;

    let page_size = u16::from_be_bytes([db_header[16], db_header[17]]);
    debug!(?page_size, "parsed db_header");
    Ok(DbHeader::new(page_size))
}

pub(crate) fn page_header(page: &[u8], num_page: usize) -> Result<PageHeader, FormatError> {
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

pub(crate) fn parse_cell(buf: &[u8]) -> Result<(TableLeafCell, usize), FormatError> {
    let mut off: usize = 0;
    debug!("start parsing a cell");
    // parsing cell
    let (payload_size, n) = varint(&buf)?;
    // offset where cell ends and starts a new one
    off += n;
    let (rowid, n) = varint(&buf[off..])?;
    off += n;

    let record_end_off = off + payload_size as usize;

    debug!(
        ?payload_size,
        record_size = payload_size + 2,
        ?rowid,
        ?off,
        ?record_end_off,
        "parsing cell hdr"
    );

    let (hdr_size, n) = varint(&buf[off..])?;
    // offset where record hdr ends
    let record_hdr_end_off = off + hdr_size as usize;
    off += n;
    let mut serial_types: Vec<SerialType> = vec![];

    debug!(?off, ?record_hdr_end_off, "start parsing columns types");
    // parsing record header
    while off < record_hdr_end_off {
        let (s_type, n) = varint(&buf[off..])?;
        let serial_type = SerialType::try_from(s_type)?;
        serial_types.push(serial_type);
        off += n;
    }
    debug!(?serial_types);

    let rec_hdr = RecordHdr::new(hdr_size as usize, serial_types);

    debug!(?rec_hdr, "start parsing column values");

    let mut values: Vec<Column> = Vec::with_capacity(rec_hdr.serial_types.len());
    for &s_type in &rec_hdr.serial_types {
        if off >= record_end_off {
            return Err(FormatError::RecordOverrun);
        }
        let (col, n) = Column::parse(&buf[off..], s_type)?;
        values.push(col);
        off += n;
    }

    info!(?values);

    let record = Record {
        hdr: rec_hdr,
        values,
    };
    let cell_parsed = TableLeafCell {
        cell_size: payload_size,
        rowid,
        record,
    };
    Ok((cell_parsed, off))
}

pub(crate) fn read_page(
    file: &File,
    buf: &mut [u8],
    page_size: u16,
    page_num: usize,
) -> Result<(), FormatError> {
    let offset = page_size as usize * page_num;
    debug!(?offset, ?page_size, ?page_num, "loading the page");
    file.read_exact_at(buf, offset as u64)?;
    Ok(())
}

pub(crate) fn varint(buf: &[u8]) -> Result<(i64, usize), FormatError> {
    const MORE_FOLLOWS: u8 = 0b1000_0000; // top bit
    const PAYLOAD: u8 = 0b0111_1111; // low 7 bits

    let mut value: u64 = 0;
    let mut i = 0;
    while i < 8 {
        let byte = *buf.get(i).ok_or_else(|| FormatError::Varint)?;
        let payload = (byte & PAYLOAD) as u64;
        value = (value << 7) | payload; // make room for 7 bits, append them
        i += 1;
        if byte & MORE_FOLLOWS == 0 {
            return Ok((value as i64, i)); // top bit clear: this was the last byte
        }
    }

    let byte = *buf.get(8).ok_or_else(|| FormatError::Varint)?;
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
    type Error = FormatError;

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
            10 | 11 => return Err(FormatError::SerialType(code)),
            n if n % 2 == 0 => Self::Blob(((n - 12) / 2) as usize),
            n => Self::Text(((n - 13) / 2) as usize),
        })
    }
}

impl TryFrom<i64> for SerialType {
    type Error = FormatError;

    fn try_from(code: i64) -> Result<Self, Self::Error> {
        SerialType::try_from(code as u64).map_err(|item| FormatError::SerialType(code as u64))
    }
}

/// One decoded column. All integer widths collapse to i64, matching sqlite's own model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Column {
    Null,
    Int(i64),
    Float(f64),
    Blob(Vec<u8>),
    Text(String),
}

impl Column {
    /// Decodes one column at the start of `buf`. Returns (column, bytes consumed).
    pub(crate) fn parse(buf: &[u8], s_type: SerialType) -> Result<(Column, usize), FormatError> {
        let n = s_type.size();

        let bytes = buf.get(..n).ok_or(FormatError::Trucated {
            need: n,
            have: buf.len(),
        })?;

        let col = match s_type {
            SerialType::Null => Self::Null,
            SerialType::Zero => Self::Int(0),
            SerialType::One => Self::Int(1),
            SerialType::I8
            | SerialType::I16
            | SerialType::I24
            | SerialType::I32
            | SerialType::I48
            | SerialType::I64 => Self::Int(int_be(bytes)),
            SerialType::F64 => Self::Float(f64::from_bits(int_be(bytes) as u64)),
            SerialType::Blob(_) => Self::Blob(bytes.to_vec()),
            SerialType::Text(_) => Self::Text(String::from_utf8(bytes.to_vec())?),
        };

        Ok((col, n))
    }
}

/// Big-endian two's-complement integer of 1..=8 bytes, sign-extended to i64.
fn int_be(bytes: &[u8]) -> i64 {
    let raw = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64);
    // Park the value in the top bits, then arithmetic-shift back down so the
    // sign bit of the original width becomes the sign bit of the i64.
    let unused = 64 - 8 * bytes.len() as u32;
    ((raw << unused) as i64) >> unused
}

/// One row: the record header's serial types applied to the body, in column order.
#[derive(Debug)]
pub(crate) struct RecordHdr {
    hdr_size: usize,
    serial_types: Vec<SerialType>,
}

impl RecordHdr {
    pub(crate) fn new(hdr_size: usize, serial_types: Vec<SerialType>) -> Self {
        Self {
            hdr_size,
            serial_types,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Record {
    pub(crate) hdr: RecordHdr,
    pub(crate) values: Vec<Column>,
}

#[derive(Debug)]
pub(crate) struct SqliteSchema {
    ty: RecordType,
    name: String,
    tbl_name: String,
    rootpage: i64,
    sql_query: String,
}
fn text(index: usize, col: Column) -> Result<String, FormatError> {
    match col {
        Column::Text(s) => Ok(s),
        got => Err(FormatError::SchemaColumn {
            index,
            expected: "Text",
            got,
        }),
    }
}

fn int(index: usize, col: Column) -> Result<i64, FormatError> {
    match col {
        Column::Int(i) => Ok(i),
        got => Err(FormatError::SchemaColumn {
            index,
            expected: "Int",
            got,
        }),
    }
}

impl SqliteSchema {
    pub(crate) fn parse(values: Vec<Column>) -> Result<SqliteSchema, FormatError> {
        let [ty, name, tbl_name, rootpage, sql_query]: [Column; 5] =
            values
                .try_into()
                .map_err(|v: Vec<Column>| FormatError::Schema {
                    exp_len: 5,
                    actual_len: v.len(),
                })?;

        let ty = match text(0, ty)?.as_str() {
            "table" => RecordType::Table,
            "index" => RecordType::Index,
            str => return Err(FormatError::UknownRecordType(str.to_owned())),
        };
        let schema = SqliteSchema {
            ty,
            name: text(1, name)?,
            tbl_name: text(2, tbl_name)?,
            rootpage: int(3, rootpage)?,
            sql_query: text(4, sql_query)?,
        };

        debug!(?schema, "parsed");

        Ok(schema)
    }

    pub(crate) fn ty(&self) -> &RecordType {
        &self.ty
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn tbl_name(&self) -> &str {
        &self.tbl_name
    }

    pub(crate) fn rootpage(&self) -> i64 {
        self.rootpage
    }

    pub(crate) fn rootpage_index(&self) -> usize {
        (self.rootpage - 1) as usize
    }

    pub(crate) fn sql_query(&self) -> &str {
        &self.sql_query
    }
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
    use super::{Column, SerialType, varint};

    #[test]
    fn column_parse() {
        assert_eq!(
            Column::parse(&[0xff], SerialType::I8).unwrap(),
            (Column::Int(-1), 1)
        );
        assert_eq!(
            Column::parse(&[0x01, 0x00], SerialType::I16).unwrap(),
            (Column::Int(256), 2)
        );
        assert_eq!(
            Column::parse(&[0x7f], SerialType::I8).unwrap(),
            (Column::Int(127), 1)
        );
        assert_eq!(
            Column::parse(b"tablexyz", SerialType::Text(5)).unwrap(),
            (Column::Text("table".into()), 5)
        );
        assert_eq!(
            Column::parse(&[], SerialType::Zero).unwrap(),
            (Column::Int(0), 0)
        );
        assert!(Column::parse(&[0x00], SerialType::I32).is_err());
    }

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
        assert_eq!(SerialType::try_from(23u64).unwrap(), SerialType::Text(5));
        assert_eq!(SerialType::try_from(27u64).unwrap(), SerialType::Text(7));
        assert_eq!(SerialType::try_from(1u64).unwrap().size(), 1);
        assert_eq!(SerialType::try_from(199u64).unwrap(), SerialType::Text(93));
        assert_eq!(SerialType::try_from(12u64).unwrap(), SerialType::Blob(0));
        assert!(SerialType::try_from(10u64).is_err());
    }
}
