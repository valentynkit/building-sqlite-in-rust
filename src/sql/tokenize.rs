use std::fmt::Display;

use tracing::{info, instrument};

use crate::{error::QueryError, helpers::QueryResult};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Keyword {
    Select,
    From,
    Where,
    Count,
}
impl Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::Select => "SELECT",
            Self::From => "FROM",
            Self::Where => "WHERE",
            Self::Count => "COUNT",
        };
        write!(f, "{out}")
    }
}

// Ident and StringLit should be seperate, ident could be case ignored, lower cased, while StringLit
// should stay exactly the same as it was

const SYMBOLS_LIST: [char; 5] = [',', '=', '(', ')', '*'];

#[derive(Debug, PartialEq, Eq)]
pub enum Symbol {
    Comma,
    Equal,
    LParen,
    RParen,
    Star,
}

impl TryFrom<char> for Symbol {
    type Error = QueryError;

    fn try_from(value: char) -> Result<Self, Self::Error> {
        let symbol = match &value {
            ',' => Self::Comma,
            '=' => Self::Equal,
            '(' => Self::LParen,
            ')' => Self::RParen,
            '*' => Self::Star,
            _ => return Err(QueryError::SymbloParsing),
        };

        Ok(symbol)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::Comma => "comma symbol",
            Self::Equal => "equal symbol",
            Self::LParen => "lef paren symbol",
            Self::RParen => "right paren symbol",
            Self::Star => "star symbol",
        };
        write!(f, "{out}")
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

impl Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyword(k) => write!(f, "{k}"),
            Self::Ident(i) => write!(f, "{i}"),
            Self::StringLit(s) => write!(f, "'{s}'"),
            Self::Symbol(s) => write!(f, "{s}"),
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
            _ => Err(()),
        }
    }
}

#[instrument(level = "info", ret, err)]
pub fn tokenize(query: &str) -> QueryResult<Vec<Token>> {
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.to_owned(),
        reason: reason.to_owned(),
    };

    let mut tokens = vec![];
    let mut rest = query.trim_start();
    while let Some(c) = rest.chars().next() {
        let (token, len) = match c {
            '\'' => {
                let close = rest[1..]
                    .find('\'')
                    .ok_or_else(|| malformed("unterminated string literal"))?;
                (
                    Token::StringLit(rest[1..=close].to_owned().into()),
                    close + 2,
                )
            }
            x if SYMBOLS_LIST.contains(&x) => (Symbol::try_from(x)?.into(), 1),
            c if is_ident_start(c) => {
                let len = rest.find(|c| !is_ident_char(c)).unwrap_or(rest.len());
                let word = &rest[..len];
                let token = Keyword::try_from(word)
                    .map_or_else(|()| Token::Ident(Ident::new(word)), Token::Keyword);
                (token, len)
            }
            other => return Err(QueryError::UknownToken(other.to_string())),
        };
        tokens.push(token);
        rest = rest[len..].trim_start();
    }
    info!("tokens count: {}", tokens.len());
    Ok(tokens)
}
