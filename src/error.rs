use std::io;

use thiserror::Error;

use crate::commands::helpers::Column;

/// Corrupt or unsupported on-disk data.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("invalid page type {0}")]
    PageType(u8),
    #[error("reserved serial type {0}")]
    SerialType(u64),
    #[error("trucated varint")]
    Varint,
    #[error("need {need} bytes, only {have} left")]
    Trucated { need: usize, have: usize },
    #[error("record body overruns its cell")]
    RecordOverrun,
    #[error("Couldn't parse record type, got: {0}")]
    UknownRecordType(String),
    #[error("text column is not utf-8")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("sqlite_schema column {index} should be {expected}, got {got:?}")]
    SchemaColumn {
        index: usize,
        expected: &'static str,
        got: Column,
    },
    #[error("uknown sql which couldn't be parsed: {got} \n expected: {expected}")]
    UknownSql { expected: String, got: String },
    #[error("sqlite_schema expected len {exp_len}, got {actual_len}")]
    Schema { exp_len: usize, actual_len: usize },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Bad or unsupported user input. sql_query.rs
#[derive(Debug, Error)]
pub enum QueryError {
    #[error("no such table: {0}")]
    NoSuchTable(String),
    #[error("no such column: {0}")]
    NoSuchColumn(String),
    #[error("{reason}; malformed query: {query}")]
    Malformed { query: String, reason: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Format(#[from] FormatError),
}
