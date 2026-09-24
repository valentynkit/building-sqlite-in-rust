use crate::{
    error::{StorageError, StorageResult},
    helpers::Column,
};

pub fn varint(buf: &[u8]) -> StorageResult<(i64, usize)> {
    const MORE_FOLLOWS: u8 = 0b1000_0000; // top bit
    const PAYLOAD: u8 = 0b0111_1111; // low 7 bits

    let mut value: u64 = 0;
    let mut i = 0;
    while i < 8 {
        let byte = *buf.get(i).ok_or(StorageError::TruncatedVarint)?;
        let payload = u64::from(byte & PAYLOAD);
        value = (value << 7) | payload; // make room for 7 bits, append them
        i += 1;
        if byte & MORE_FOLLOWS == 0 {
            return Ok((value.cast_signed(), i)); // top bit clear: this was the last byte
        }
    }

    let byte = *buf.get(8).ok_or(StorageError::TruncatedVarint)?;
    value = (value << 8) | u64::from(byte);
    Ok((value.cast_signed(), 9))
}

/// Big-endian `u32` at `at`, the width of every page number in the format.
pub fn read_u32(buf: &[u8], at: usize) -> StorageResult<u32> {
    buf.get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or(StorageError::Truncated {
            need: at + 4,
            have: buf.len(),
        })
}

/// A varint used as a length or offset; negative means the file is corrupt.
pub fn to_usize(value: i64, what: &'static str) -> StorageResult<usize> {
    usize::try_from(value).map_err(|_| StorageError::OutOfRange { what, value })
}

pub fn text(index: usize, col: Column) -> StorageResult<String> {
    match col {
        Column::Text(s) => Ok(s),
        found => Err(StorageError::SchemaColumnType {
            index,
            expected: "text",
            found,
        }),
    }
}

pub fn int(index: usize, col: Column) -> StorageResult<i64> {
    match col {
        Column::Int(i) => Ok(i),
        found => Err(StorageError::SchemaColumnType {
            index,
            expected: "integer",
            found,
        }),
    }
}

/// Big-endian two's-complement integer of 1..=8 bytes, sign-extended to i64.
pub fn int_be(bytes: &[u8]) -> i64 {
    debug_assert!(
        (1..=8).contains(&bytes.len()),
        "serial types are 1..=8 bytes"
    );
    let raw = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
    // Park the value in the top bits, then arithmetic-shift back down so the
    // sign bit of the original width becomes the sign bit of the i64.
    #[allow(clippy::cast_possible_truncation)] // len <= 8
    let unused = 64 - 8 * bytes.len() as u32;
    (raw << unused).cast_signed() >> unused
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
