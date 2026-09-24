//! Errors are split by who is at fault, not by which module raised them.
//!
//! - [`StorageError`]: the file can't be read, or its bytes aren't a database we understand.
//! - [`QueryError`]: the SQL text is wrong, or names something that doesn't exist.
//!
//! Both are plain `thiserror` enums so callers can match on them. `anyhow` appears only in
//! `lib.rs`/`main.rs`, where errors get context and are printed. Errors are built silently and
//! logged once, by `#[instrument(err)]` on the command entry points, never at the raise site.

use std::{io, string::FromUtf8Error};

use thiserror::Error;

use crate::{
    helpers::Column,
    sql::{Ident, Token},
};

pub type StorageResult<T> = Result<T, StorageError>;
pub type QueryResult<T> = Result<T, QueryError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("not a SQLite 3 database (bad header magic)")]
    NotADatabase,

    #[error("failed to read {len} bytes at offset {offset}")]
    Read {
        offset: u64,
        len: usize,
        #[source]
        source: io::Error,
    },

    #[error("invalid b-tree page type {0:#04x}")]
    InvalidPageType(u8),

    #[error("need {need} bytes, only {have} available")]
    Truncated { need: usize, have: usize },

    #[error("varint runs past the end of its buffer")]
    TruncatedVarint,

    #[error("reserved serial type {0}")]
    ReservedSerialType(u64),

    #[error("record body overruns its cell")]
    RecordOverrun,

    #[error("record has {len} columns, column {column} requested")]
    RecordTooShort { column: usize, len: usize },

    #[error("text column is not valid UTF-8")]
    InvalidUtf8(#[from] FromUtf8Error),

    #[error("{what} out of range: {value}")]
    OutOfRange { what: &'static str, value: i64 },

    #[error("sqlite_schema row has {0} columns, expected 5")]
    SchemaRowLen(usize),

    #[error("sqlite_schema column {index} should be {expected}, found {found:?}")]
    SchemaColumnType {
        index: usize,
        expected: &'static str,
        found: Column,
    },

    #[error("unknown sqlite_schema object type `{0}`")]
    UnknownObjectType(String),

    #[error("unsupported schema SQL ({reason}): {sql}")]
    UnsupportedSchemaSql { reason: &'static str, sql: String },

    #[error("table `{0}` is defined more than once in sqlite_schema")]
    DuplicateTable(Ident),
}

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("unexpected character `{ch}` at position {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    #[error("unterminated string literal starting at position {pos}")]
    UnterminatedString { pos: usize },

    #[error("expected {expected}, found {found}")]
    UnexpectedToken {
        expected: &'static str,
        found: Token,
    },

    #[error("expected {expected}, found end of query")]
    UnexpectedEnd { expected: &'static str },

    #[error("no such table: {0}")]
    NoSuchTable(Ident),

    #[error("no such column: {0}")]
    NoSuchColumn(Ident),

    #[error(transparent)]
    Storage(#[from] StorageError),
}
