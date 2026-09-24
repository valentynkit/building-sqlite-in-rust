use std::fmt::Display;

use tracing::{debug, instrument};

use crate::error::{QueryError, QueryResult};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Keyword {
    Select,
    From,
    Where,
    Count,
}
impl Keyword {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::From => "FROM",
            Self::Where => "WHERE",
            Self::Count => "COUNT",
        }
    }
}

impl Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// Ident and StringLit should be seperate, ident could be case ignored, lower cased, while StringLit
// should stay exactly the same as it was

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Comma,
    Equal,
    LParen,
    RParen,
    Star,
}

impl TryFrom<char> for Symbol {
    type Error = ();

    fn try_from(value: char) -> Result<Self, ()> {
        match value {
            ',' => Ok(Self::Comma),
            '=' => Ok(Self::Equal),
            '(' => Ok(Self::LParen),
            ')' => Ok(Self::RParen),
            '*' => Ok(Self::Star),
            _ => Err(()),
        }
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Comma => ",",
            Self::Equal => "=",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::Star => "*",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct StringLit(String);

impl Display for StringLit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StringLit {
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for StringLit {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for StringLit {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Ident(String);
impl Ident {
    pub fn new(value: impl Into<String>) -> Self {
        let mut s = value.into();
        s.make_ascii_lowercase();
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<String> for Ident {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl From<String> for Ident {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Ident {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Token {
    Keyword(Keyword),
    Ident(Ident),
    StringLit(StringLit),
    Symbol(Symbol),
}

impl From<Symbol> for Token {
    fn from(value: Symbol) -> Self {
        Self::Symbol(value)
    }
}

/// Written for error messages: `expected FROM, found identifier `name``.
impl Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyword(k) => write!(f, "keyword {k}"),
            Self::Ident(i) => write!(f, "identifier `{i}`"),
            Self::StringLit(s) => write!(f, "string '{s}'"),
            Self::Symbol(s) => write!(f, "`{s}`"),
        }
    }
}

const fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl TryFrom<&str> for Keyword {
    type Error = ();
    fn try_from(word: &str) -> Result<Self, ()> {
        match word.to_ascii_lowercase().as_str() {
            "select" => Ok(Self::Select),
            "from" => Ok(Self::From),
            "where" => Ok(Self::Where),
            "count" => Ok(Self::Count),
            _ => Err(()),
        }
    }
}

#[instrument(level = "debug", ret)]
pub fn tokenize(query: &str) -> QueryResult<Vec<Token>> {
    let mut tokens = vec![];
    let mut rest = query.trim_start();
    while let Some(c) = rest.chars().next() {
        let pos = query.len() - rest.len();
        let (token, len) = match c {
            '\'' => {
                let close = rest[1..]
                    .find('\'')
                    .ok_or(QueryError::UnterminatedString { pos })?;
                (
                    Token::StringLit(rest[1..=close].to_owned().into()),
                    close + 2,
                )
            }
            c if is_ident_start(c) => {
                let len = rest.find(|c| !is_ident_char(c)).unwrap_or(rest.len());
                let word = &rest[..len];
                let token = Keyword::try_from(word)
                    .map_or_else(|()| Token::Ident(Ident::new(word)), Token::Keyword);
                (token, len)
            }
            c => {
                let symbol =
                    Symbol::try_from(c).map_err(|()| QueryError::UnexpectedChar { ch: c, pos })?;
                (symbol.into(), 1)
            }
        };
        tokens.push(token);
        rest = rest[len..].trim_start();
    }
    debug!(count = tokens.len(), "tokenized");
    Ok(tokens)
}
