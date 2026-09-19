use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use tracing::{debug, warn};

use crate::{
    error::{FormatError, QueryError},
    helpers::{
        Column, QueryResult, RecordType, Result, SqliteSchema, int, page_header, parse_leaf_cell,
        text,
    },
    sql::{Ident, Keyword, StringLit, Symbol, Token},
};

/// Used for parsing, during iteration to identify what is the current query section.
#[derive(Debug, PartialEq, Eq)]
pub enum QuerySection {
    Unstarted,
    Select,
    From,
    Where,
}

impl Default for QuerySection {
    fn default() -> Self {
        Self::Unstarted
    }
}

impl Display for QuerySection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::Unstarted => "Unstarted",
            Self::Select => "Select",
            Self::From => "From",
            Self::Where => "Where",
        };
        write!(f, "{out}")
    }
}

impl QuerySection {
    // State machine, moving to next query section
    fn next(self) -> QueryResult<Self> {
        let res = match self {
            Self::Unstarted => Self::Select,
            Self::Select => Self::From,
            Self::From => Self::Where,
            Self::Where => return Err(QueryError::QuerySection(Self::Where)),
        };
        Ok(res)
    }

    fn try_progress_to_next_section(self, token: Keyword) -> QueryResult<Self> {
        let expected: Keyword = match self {
            Self::Unstarted => Keyword::Select,
            Self::Select => Keyword::From,
            Self::From => Keyword::Where,
            Self::Where => {
                return Err(QueryError::QuerySection(Self::Where));
            }
        };

        if token != expected {
            return Err(QueryError::Parser {
                token: Token::Keyword(token),
                reason: format!("Where section shouldn't contain another keyword"),
            });
        }
        self.next()
    }
}

#[derive(Debug)]
pub struct ParsedTokens {
    pub what: Vec<Ident>,
    pub from: Vec<Ident>,
    pub conditions: Vec<Condition>,
}

#[derive(Debug)]
struct Condition {
    column_name: Ident,
    symbol: Symbol,
    exp_value: StringLit,
}

impl Condition {
    fn new(column_name: Ident, symbol: Symbol, exp_value: StringLit) -> Self {
        Self {
            column_name,
            symbol,
            exp_value,
        }
    }
}

// TODO: probably cleaner would be not passing query at all, but instead propogate some error, and
// let the caller parse this error, and throw a new one by enriching it with query, like the
// Malformed type, whire this being agnorant of the actual query passed to it.
pub fn parse_query(query: &str, tokens: Vec<Token>) -> QueryResult<ParsedTokens> {
    let mut query_section = QuerySection::default();

    let mut what: Vec<Ident> = vec![];
    let mut from: Vec<Ident> = vec![];
    let mut conditions: Vec<Condition> = vec![];

    let malformed = |reason: &str| QueryError::Malformed {
        query: query.to_owned(),
        reason: reason.to_owned(),
    };
    let mut tokens = tokens.into_iter();
    while let Some(token) = tokens.next() {
        match query_section {
            QuerySection::Unstarted => {
                let Token::Keyword(keyword) = token else {
                    return Err(malformed("expected to start with keyword"));
                };

                query_section = query_section.try_progress_to_next_section(keyword)?;
            }
            QuerySection::Select => match token {
                Token::Keyword(keyword) => {
                    query_section = query_section.try_progress_to_next_section(keyword)?;
                }
                Token::Ident(ident) => {
                    what.push(ident);
                }
                _ => {
                    return Err(malformed(
                        "expected having identifiers or FROM keyword in SELECT section",
                    ));
                }
            },
            QuerySection::From => match token {
                Token::Keyword(keyword) => {
                    query_section = query_section.try_progress_to_next_section(keyword)?;
                    break;
                }
                Token::Ident(ident) => {
                    from.push(ident);
                }
                _ => {
                    return Err(malformed(
                        "expected having identifiers or WHERE keyword in FROM section",
                    ));
                }
            },
            QuerySection::Where => {
                return Err(malformed(
                    "where section shouldn't be reached in per token parsing, and should be handled seperately",
                ));
            }
        }
    }

    if query_section == QuerySection::Where {
        loop {
            let (Some(col), Some(sym), Some(val)) = (tokens.next(), tokens.next(), tokens.next())
            else {
                return Err(malformed("WHERE expects triple tuple `<col> <op> <value>`"));
            };
            let (
                Token::Ident(column_name),
                Token::Symbol(Symbol::Equal),
                Token::StringLit(exp_value),
            ) = (col, sym, val)
            else {
                return Err(malformed("WHERE expects triple tuple `<col> = <value>`"));
            };

            let condition = Condition::new(column_name, Symbol::Equal, exp_value);
            debug!(?condition, "parsed query chunk condition");
            conditions.push(condition);
        }
    }

    let parsed_tokens = ParsedTokens {
        what,
        from,
        conditions,
    };
    debug!(?parsed_tokens, "parsed SQL query");
    Ok(parsed_tokens)
}

fn parse_index_sql(
    query: &str,
    uknown_sql: &impl Fn(&str, &str) -> FormatError,
) -> Result<RecordType> {
    let open = query.find('(').ok_or_else(|| uknown_sql("(", ""))?;
    let close = query.find(')').ok_or_else(|| uknown_sql(")", ""))?;

    if open >= close {
        return Err(uknown_sql(
            "(...)",
            &format!("`(` at {open} after `)` at {close}"),
        ));
    }

    let inside_parentheses = &query[open + 1..close];
    Ok(RecordType::Index {
        col_name: inside_parentheses.to_owned(),
    })
}

fn parse_table_sql(
    query: &str,
    uknown_sql: &impl Fn(&str, &str) -> FormatError,
) -> Result<RecordType> {
    let open = query.find('(').ok_or_else(|| uknown_sql("(", ""))?;
    let close = query.find(')').ok_or_else(|| uknown_sql(")", ""))?;

    if open >= close {
        return Err(uknown_sql(
            "(...)",
            &format!("`(` at {open} after `)` at {close}"),
        ));
    }

    let inside_parentheses = &query[open + 1..close];

    debug!(?inside_parentheses);
    let mut parsed_columns: HashMap<String, usize> = HashMap::new();
    let mut rowid_alias = None;
    let columns: Vec<&str> = inside_parentheses.split(',').map(str::trim).collect();

    debug!(?columns);
    for (idx, sub_str) in columns.iter().enumerate() {
        let item = sub_str
            .split_whitespace()
            .next()
            .ok_or(FormatError::UknownSql {
                expected: "column name".to_owned(),
                got: "None".to_owned(),
            })?;
        parsed_columns.insert(item.to_owned(), idx);
        if sub_str.contains("integer primary key") {
            rowid_alias = Some(idx);
        }
    }

    debug!(?parsed_columns, ?rowid_alias);
    let rec = RecordType::Table {
        parsed_columns,
        rowid_alias,
    };

    Ok(rec)
}

pub(crate) fn parse(values: Vec<Column>) -> Result<SqliteSchema> {
    let [ty, _, tbl_name, rootpage, sql_query]: [Column; 5] =
        values
            .try_into()
            .map_err(|v: Vec<Column>| FormatError::Schema {
                exp_len: 5,
                actual_len: v.len(),
            })?;

    let sql_query = text(4, sql_query)?;
    let tbl_name = text(2, tbl_name)?;

    let query = sql_query.to_ascii_lowercase();

    let uknown_sql = |expected: &str, got: &str| FormatError::UknownSql {
        expected: expected.to_owned(),
        got: got.to_owned(),
    };

    let query_start = format!("create {ty}");
    if !query.starts_with(query_start.as_str()) {
        return Err(FormatError::UknownSql {
            got: query,
            expected: query_start,
        });
    }
    debug!(?query, ?tbl_name, "parsed sql");

    let ty = match text(0, ty)?.as_str() {
        "table" => parse_table_sql(&query, &uknown_sql),
        "index" => parse_index_sql(&query, &uknown_sql),
        str => Err(FormatError::UknownRecordType(str.to_owned())),
    }?;

    let schema = SqliteSchema::new(ty, tbl_name, int(3, rootpage)?);
    /*
                name: text(1, name)?,
                sql_query,
    */

    debug!(?schema, "parsed");

    Ok(schema)
}

pub fn parse_sqlite_schemas(page_buf: &[u8]) -> anyhow::Result<Vec<SqliteSchema>> {
    let page_header = page_header(page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        debug!(offset = ptr, "start parsing cell");
        let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
        schemas.push(parse(cell.record.values)?);
    }

    Ok(schemas)
}
