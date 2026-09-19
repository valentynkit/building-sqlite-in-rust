use crate::{
    error::{FormatError, QueryError},
    helpers::Column,
};

pub type Result<T, E = FormatError> = core::result::Result<T, E>;
pub type QueryResult<T, E = QueryError> = core::result::Result<T, E>;

#[derive(Copy, Clone)]
pub struct DbHeader {
    page_size: u16,
}

impl DbHeader {
    pub const fn new(page_size: u16) -> Self {
        Self { page_size }
    }
    pub const fn page_size(self) -> u16 {
        self.page_size
    }
}

pub fn varint(buf: &[u8]) -> Result<(i64, usize)> {
    const MORE_FOLLOWS: u8 = 0b1000_0000; // top bit
    const PAYLOAD: u8 = 0b0111_1111; // low 7 bits

    let mut value: u64 = 0;
    let mut i = 0;
    while i < 8 {
        let byte = *buf.get(i).ok_or(FormatError::Varint)?;
        let payload = u64::from(byte & PAYLOAD);
        value = (value << 7) | payload; // make room for 7 bits, append them
        i += 1;
        if byte & MORE_FOLLOWS == 0 {
            return Ok((value.cast_signed(), i)); // top bit clear: this was the last byte
        }
    }

    let byte = *buf.get(8).ok_or(FormatError::Varint)?;
    value = (value << 8) | u64::from(byte);
    Ok((value.cast_signed(), 9))
}

pub fn text(index: usize, col: Column) -> Result<String> {
    match col {
        Column::Text(s) => Ok(s),
        got => Err(FormatError::SchemaColumn {
            index,
            expected: "Text",
            got,
        }),
    }
}

pub fn int(index: usize, col: Column) -> Result<i64> {
    match col {
        Column::Int(i) => Ok(i),
        got => Err(FormatError::SchemaColumn {
            index,
            expected: "Int",
            got,
        }),
    }
}

/// Big-endian two's-complement integer of 1..=8 bytes, sign-extended to i64.
pub fn int_be(bytes: &[u8]) -> Result<i64> {
    let raw = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
    // Park the value in the top bits, then arithmetic-shift back down so the
    // sign bit of the original width becomes the sign bit of the i64.
    let unused = u32::try_from((64 - 8) * bytes.len())?;
    Ok((raw << unused).cast_signed() >> unused)
}

#[test]
fn varint_spec_examples() {
    assert_eq!(varint(&[0x2a]).unwrap(), (42, 1));
    assert_eq!(varint(&[0xb3, 0x33]).unwrap(), (6579, 2));
    assert_eq!(varint(&[0x81, 0xbf, 0x81, 0x3f]).unwrap(), (3_129_535, 4));
    // trailing bytes ignored, only consumed count matters
    assert_eq!(varint(&[0x78, 0x03, 0x07]).unwrap(), (120, 1));
    assert!(varint(&[0x80]).is_err());
}
