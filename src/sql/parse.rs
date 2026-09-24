use std::{collections::HashMap, fmt::Display, fs::File};

use tracing::{debug, instrument};

use crate::{
    error::{FormatError, QueryError},
    helpers::{
        Column, QueryResult, RecordType, Result, SqliteSchema, int, page_header, page_size,
        parse_leaf_cell, read_page, text,
    },
    sql::{Ident, Keyword, Symbol, Token},
};

/// Used for parsing, during iteration to identify what is the current query section.
#[derive(Debug, PartialEq, Eq, Default)]
pub enum QuerySection {
    #[default]
    Unstarted,
    Select,
    From,
    Where,
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
    const fn next(self) -> QueryResult<Self> {
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
                reason: "Where section shouldn't contain another keyword".to_string(),
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
pub struct Condition {
    pub column_name: Ident,
    pub _symbol: Symbol,
    pub exp_value: Column,
}

impl Condition {
    const fn new(column_name: Ident, _symbol: Symbol, exp_value: Column) -> Self {
        Self {
            column_name,
            _symbol,
            exp_value,
        }
    }
}

// TODO: probably cleaner would be not passing query at all, but instead propogate some error, and
// let the caller parse this error, and throw a new one by enriching it with query, like the
// Malformed type, whire this being agnorant of the actual query passed to it.

#[instrument(level = "info", skip(tokens), ret, err)]
pub fn parse_query(tokens: Vec<Token>) -> QueryResult<ParsedTokens> {
    let mut query_section = QuerySection::default();

    let mut what: Vec<Ident> = vec![];
    let mut from: Vec<Ident> = vec![];
    let mut conditions: Vec<Condition> = vec![];

    let malformed = |token: Token, reason: &str| QueryError::Parser {
        token: token,
        reason: reason.to_owned(),
    };
    let mut tokens = tokens.into_iter();
    for token in tokens.by_ref() {
        match query_section {
            QuerySection::Unstarted => {
                let Token::Keyword(keyword) = token else {
                    return Err(malformed(token, "expected to start with keyword"));
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
                Token::Symbol(Symbol::Comma) => {}
                _ => {
                    return Err(malformed(
                        token,
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
                        token,
                        "expected having identifiers or WHERE keyword in FROM section",
                    ));
                }
            },
            QuerySection::Where => {
                return Err(malformed(
                    token,
                    "where section shouldn't be reached in per token parsing, and should be handled seperately",
                ));
            }
        }
    }

    if query_section == QuerySection::Where {
        loop {
            let (Some(col), Some(sym), Some(val)) = (tokens.next(), tokens.next(), tokens.next())
            else {
                return Err(QueryError::InternalTokensParser {
                    reason: "WHERE expects triple tuple `<col> <op> <value>`".to_string(),
                });
            };
            let (
                Token::Ident(column_name),
                Token::Symbol(Symbol::Equal),
                Token::StringLit(exp_value),
            ) = (col, sym, val)
            else {
                return Err(QueryError::InternalTokensParser {
                    reason: "WHERE expects triple tuple `<col> = <value>`".to_string(),
                });
            };

            let condition = Condition::new(
                column_name,
                Symbol::Equal,
                Column::Text(exp_value.into_inner()),
            );
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
        col_name: Ident::new(inside_parentheses),
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
    let mut parsed_columns: HashMap<Ident, usize> = HashMap::new();
    let mut rowid_alias = None;
    let columns: Vec<&str> = inside_parentheses.split(',').map(str::trim).collect();

    debug!(?columns);
    for (idx, sub_str) in columns.iter().enumerate() {
        let item = sub_str
            .split_whitespace()
            .next()
            .ok_or_else(|| FormatError::UknownSql {
                expected: "column name".to_owned(),
                got: "None".to_owned(),
            })?;
        parsed_columns.insert(Ident::new(item), idx);
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

pub fn parse(values: Vec<Column>) -> Result<SqliteSchema> {
    let [ty, _, tbl_name, rootpage, sql_query]: [Column; 5] =
        values
            .try_into()
            .map_err(|v: Vec<Column>| FormatError::Schema {
                exp_len: 5,
                actual_len: v.len(),
            })?;

    let sql_query = text(4, sql_query)?;
    let tbl_name = Ident::new(text(2, tbl_name)?);

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

#[instrument(level = "debug", skip(file), err)]
pub fn parse_first_page(file: &File) -> anyhow::Result<Vec<u8>> {
    let page_size = page_size(file)?;

    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, 0)?;
    Ok(page_buf)
}

#[instrument(skip(page_buf), err)]
pub fn parse_sqlite_schemas(page_buf: &[u8]) -> anyhow::Result<Vec<SqliteSchema>> {
    let page_header = page_header(page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
        debug!(?cell);
        schemas.push(parse(cell.record.values)?);
    }
    debug!("successfully parsed sqlite schemas: {}", schemas.len());

    Ok(schemas)
}
