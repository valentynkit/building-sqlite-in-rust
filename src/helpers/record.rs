// VARINT, Serial Types, Column

use super::Result;
use std::{collections::HashMap, fmt::Display};

use crate::{error::FormatError, helpers::int_be};

/// Record-format serial type. Spec: `<https://www.sqlite.org/fileformat.html#record_format>`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialType {
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
    pub const fn size(self) -> usize {
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
        Self::try_from(code.cast_unsigned())
            .map_err(|_| FormatError::SerialType(code.cast_unsigned()))
    }
}

/// One decoded column. All integer widths collapse to i64, matching sqlite's own model.
#[derive(Debug, Clone, PartialEq)]
pub enum Column {
    Null,
    Int(i64),
    Float(f64),
    Blob(Vec<u8>),
    Text(String),
}

impl Display for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(float) => write!(f, "{float}"),
            Self::Text(s) => f.write_str(s),
            Self::Blob(b) => write!(f, "<blob {} bytes>", b.len()),
        }
    }
}

impl Column {
    /// Decodes one column at the start of `buf`. Returns (column, bytes consumed).
    pub fn parse(buf: &[u8], s_type: SerialType) -> Result<(Self, usize)> {
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
            | SerialType::I64 => Self::Int(int_be(bytes)?),
            SerialType::F64 => Self::Float(f64::from_bits(int_be(bytes)?.cast_unsigned())),
            SerialType::Blob(_) => Self::Blob(bytes.to_vec()),
            SerialType::Text(_) => Self::Text(String::from_utf8(bytes.to_vec())?),
        };

        Ok((col, n))
    }
}

/// One row: the record header's serial types applied to the body, in column order.
#[derive(Debug)]
pub struct RecordHdr {
    serial_types: Vec<SerialType>,
}

impl RecordHdr {
    pub(crate) fn new(serial_types: Vec<SerialType>) -> Self {
        Self { serial_types }
    }

    pub fn serial_types(&self) -> &[SerialType] {
        &self.serial_types
    }
}

#[derive(Debug)]
pub(crate) struct Record {
    pub(crate) values: Vec<Column>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecordType {
    Table {
        // map of columns name to index from sql query, so we could use it when parsing the rows and
        // getting specific columns by index having only the names of columns.
        parsed_columns: HashMap<String, usize>,
        // `integer primary key` column: stored as NULL in the record, its value is the rowid.
        rowid_alias: Option<usize>,
    },
    Index {
        col_name: String,
    },
}

impl Display for RecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table {
                parsed_columns: _parsed_columns,
                rowid_alias: _rowid_alias,
            } => write!(f, "table"),
            Self::Index {
                col_name: _col_name,
            } => write!(f, "index"),
        }
    }
}

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
fn serial_type_matches_spec_example() {
    // From the sample.db "oranges" cell: 17 1b 1b 01 81 47
    assert_eq!(SerialType::try_from(23u64).unwrap(), SerialType::Text(5));
    assert_eq!(SerialType::try_from(27u64).unwrap(), SerialType::Text(7));
    assert_eq!(SerialType::try_from(1u64).unwrap().size(), 1);
    assert_eq!(SerialType::try_from(199u64).unwrap(), SerialType::Text(93));
    assert_eq!(SerialType::try_from(12u64).unwrap(), SerialType::Blob(0));
    assert!(SerialType::try_from(10u64).is_err());
}
