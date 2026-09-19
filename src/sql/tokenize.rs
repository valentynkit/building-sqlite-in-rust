use std::fmt::Display;

use crate::{error::QueryError, helpers::QueryResult};

/// Splits on whitespace, except a single-quoted literal stays one token, quotes included.
/// "a = 'New York'" -> ["a", "=", "'New York'"]

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Keyword {
    Select,
    From,
    Where,
}
impl Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::Select => "SELECT",
            Self::From => "FROM",
            Self::Where => "WHERE",
        };
        write!(f, "{out}")
    }
}

// Ident and StringLit should be seperate, ident could be case ignored, lower cased, while StringLit
// should stay exactly the same as it was

#[derive(Debug, PartialEq, Eq)]
pub enum Symbol {
    Comma,
    Equal,
}

impl Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::Comma => "comma symbol",
            Self::Equal => "equal symbol",
        };
        write!(f, "{out}")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct StringLit(String);

impl From<String> for StringLit {
    fn from(value: String) -> Self {
        StringLit(value)
    }
}

impl From<&str> for StringLit {
    fn from(value: &str) -> Self {
        StringLit::from(value.to_owned())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ident(String);
impl From<String> for Ident {
    fn from(value: String) -> Self {
        Ident(value)
    }
}

impl From<&str> for Ident {
    fn from(value: &str) -> Self {
        Ident::from(value.to_owned())
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

impl Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
fn is_ident(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_char)
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

pub fn tokenize(query: &str) -> QueryResult<Vec<Token>> {
    let malformed = |reason: &str| QueryError::Malformed {
        query: query.to_owned(),
        reason: reason.to_owned(),
    };

    let mut tokens = vec![];
    let mut rest = query.trim_start();
    while let Some(c) = rest.chars().next() {
        let (token, len) = match c {
            ',' => (Token::Symbol(Symbol::Comma), 1),
            '=' => (Token::Symbol(Symbol::Equal), 1),
            '\'' => {
                let close = rest[1..]
                    .find('\'')
                    .ok_or_else(|| malformed("unterminated string literal"))?;
                (
                    Token::StringLit(rest[1..=close].to_owned().into()),
                    close + 2,
                )
            }
            c if is_ident_start(c) => {
                let len = rest.find(|c| !is_ident_char(c)).unwrap_or(rest.len());
                let word = &rest[..len];
                let token = Keyword::try_from(word)
                    .map_or_else(|()| Token::Ident(word.to_owned().into()), Token::Keyword);
                (token, len)
            }
            other => return Err(QueryError::UknownToken(other.to_string())),
        };
        tokens.push(token);
        rest = rest[len..].trim_start();
    }
    Ok(tokens)
}
