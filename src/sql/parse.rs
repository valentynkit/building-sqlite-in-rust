use std::{collections::HashMap, fmt::Display, fs::File};

use tracing::{debug, instrument};

use crate::{
    error::{QueryError, QueryResult, StorageError, StorageResult},
    helpers::{
        Column, RecordType, SqliteSchema, int, page_header, page_size, parse_leaf_cell, read_page,
        text,
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
    /// Moves to the next clause if `keyword` is the one that opens it.
    fn advance(self, keyword: Keyword) -> QueryResult<Self> {
        let (expected, next) = match self {
            Self::Unstarted => (Keyword::Select, Self::Select),
            Self::Select => (Keyword::From, Self::From),
            Self::From => (Keyword::Where, Self::Where),
            Self::Where => {
                return Err(unexpected("end of query", Token::Keyword(keyword)));
            }
        };
        if keyword != expected {
            return Err(unexpected(expected.as_str(), Token::Keyword(keyword)));
        }
        Ok(next)
    }
}

const fn unexpected(expected: &'static str, found: Token) -> QueryError {
    QueryError::UnexpectedToken { expected, found }
}

fn next_token(
    tokens: &mut impl Iterator<Item = Token>,
    expected: &'static str,
) -> QueryResult<Token> {
    tokens.next().ok_or(QueryError::UnexpectedEnd { expected })
}

fn expect_symbol(
    tokens: &mut impl Iterator<Item = Token>,
    symbol: Symbol,
    expected: &'static str,
) -> QueryResult<()> {
    match next_token(tokens, expected)? {
        Token::Symbol(s) if s == symbol => Ok(()),
        found => Err(unexpected(expected, found)),
    }
}

#[derive(Debug)]
pub enum Projection {
    Count,
    Columns(Vec<Ident>),
}
#[derive(Debug)]
pub struct ParsedTokens {
    pub projection: Projection,
    pub table: Ident,
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

#[instrument(level = "debug", skip(tokens), ret)]
pub fn parse_query(tokens: Vec<Token>) -> QueryResult<ParsedTokens> {
    const PROJECTION: &str = "a column name or COUNT(*)";

    let mut query_section = QuerySection::default();
    let mut projection: Option<Projection> = None;
    // True at the start of the SELECT list and after each comma.
    let mut needs_column = true;
    let mut table: Option<Ident> = None;
    let mut conditions: Vec<Condition> = vec![];

    let mut tokens = tokens.into_iter();
    while let Some(token) = tokens.next() {
        match query_section {
            QuerySection::Unstarted => {
                let Token::Keyword(keyword) = token else {
                    return Err(unexpected("SELECT", token));
                };
                query_section = query_section.advance(keyword)?;
            }
            QuerySection::Select => match token {
                Token::Keyword(Keyword::Count) if projection.is_none() => {
                    expect_symbol(&mut tokens, Symbol::LParen, "`(` after COUNT")?;
                    expect_symbol(&mut tokens, Symbol::Star, "`*` in COUNT(*)")?;
                    expect_symbol(&mut tokens, Symbol::RParen, "`)` after COUNT(*")?;
                    projection = Some(Projection::Count);
                    needs_column = false;
                }
                Token::Ident(ident) if needs_column => {
                    needs_column = false;
                    match projection.get_or_insert_with(|| Projection::Columns(vec![])) {
                        Projection::Columns(columns) => columns.push(ident),
                        Projection::Count => unreachable!("COUNT(*) clears needs_column"),
                    }
                }
                Token::Symbol(Symbol::Comma)
                    if !needs_column && matches!(projection, Some(Projection::Columns(_))) =>
                {
                    needs_column = true;
                }
                Token::Keyword(keyword) if !needs_column => {
                    query_section = query_section.advance(keyword)?;
                }
                found if projection.is_none() => return Err(unexpected(PROJECTION, found)),
                found if needs_column => return Err(unexpected("a column name", found)),
                found if matches!(projection, Some(Projection::Count)) => {
                    return Err(unexpected("FROM", found));
                }
                found => return Err(unexpected("`,` or FROM", found)),
            },
            QuerySection::From => match token {
                Token::Ident(ident) if table.is_none() => table = Some(ident),
                Token::Keyword(keyword) if table.is_some() => {
                    query_section = query_section.advance(keyword)?;
                    break;
                }
                found if table.is_none() => return Err(unexpected("a table name", found)),
                found => return Err(unexpected("WHERE or end of query", found)),
            },
            QuerySection::Where => unreachable!("WHERE is parsed after this loop"),
        }
    }

    if query_section == QuerySection::Where {
        loop {
            let column_name = match next_token(&mut tokens, "a column name")? {
                Token::Ident(ident) => ident,
                found => return Err(unexpected("a column name", found)),
            };
            expect_symbol(&mut tokens, Symbol::Equal, "`=`")?;
            let exp_value = match next_token(&mut tokens, "a string literal")? {
                Token::StringLit(lit) => Column::Text(lit.into_inner()),
                found => return Err(unexpected("a string literal", found)),
            };
            let condition = Condition::new(column_name, Symbol::Equal, exp_value);
            debug!(?condition, "parsed condition");
            conditions.push(condition);
            if tokens.len() == 0 {
                break;
            }
        }
    }

    let projection = projection.ok_or(QueryError::UnexpectedEnd {
        expected: PROJECTION,
    })?;
    let table = table.ok_or(QueryError::UnexpectedEnd {
        expected: "FROM <table>",
    })?;

    Ok(ParsedTokens {
        projection,
        table,
        conditions,
    })
}

/// Text between the first `(` and the last `)` of a CREATE statement.
fn between_parens(sql: &str) -> StorageResult<&str> {
    let unsupported = |reason| StorageError::UnsupportedSchemaSql {
        reason,
        sql: sql.to_owned(),
    };
    let open = sql.find('(').ok_or_else(|| unsupported("missing `(`"))?;
    let close = sql.rfind(')').ok_or_else(|| unsupported("missing `)`"))?;
    if close < open {
        return Err(unsupported("`)` before `(`"));
    }
    Ok(&sql[open + 1..close])
}

fn parse_index_sql(sql: &str) -> StorageResult<RecordType> {
    Ok(RecordType::Index {
        col_name: Ident::new(between_parens(sql)?.trim()),
    })
}

fn parse_table_sql(sql: &str) -> StorageResult<RecordType> {
    let mut parsed_columns: HashMap<Ident, usize> = HashMap::new();
    let mut rowid_alias = None;

    for (idx, definition) in between_parens(sql)?.split(',').map(str::trim).enumerate() {
        let name = definition.split_whitespace().next().ok_or_else(|| {
            StorageError::UnsupportedSchemaSql {
                reason: "empty column definition",
                sql: sql.to_owned(),
            }
        })?;
        parsed_columns.insert(Ident::new(name), idx);
        if definition.contains("integer primary key") {
            rowid_alias = Some(idx);
        }
    }

    debug!(?parsed_columns, ?rowid_alias);
    Ok(RecordType::Table {
        parsed_columns,
        rowid_alias,
    })
}

pub fn parse(values: Vec<Column>) -> StorageResult<SqliteSchema> {
    let [ty, _, tbl_name, rootpage, sql]: [Column; 5] = values
        .try_into()
        .map_err(|v: Vec<Column>| StorageError::SchemaRowLen(v.len()))?;

    let ty = text(0, ty)?;
    let tbl_name = Ident::new(text(2, tbl_name)?);
    let sql = text(4, sql)?.to_ascii_lowercase();

    if !sql.starts_with(&format!("create {ty}")) {
        return Err(StorageError::UnsupportedSchemaSql {
            reason: "does not start with CREATE <type>",
            sql,
        });
    }

    let ty = match ty.as_str() {
        "table" => parse_table_sql(&sql)?,
        "index" => parse_index_sql(&sql)?,
        _ => return Err(StorageError::UnknownObjectType(ty)),
    };

    let schema = SqliteSchema::new(ty, tbl_name, int(3, rootpage)?);
    debug!(?schema, "parsed");
    Ok(schema)
}

#[instrument(level = "debug", skip(file))]
pub fn parse_first_page(file: &File) -> StorageResult<Vec<u8>> {
    let page_size = page_size(file)?;
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(file, &mut page_buf, page_size, 0)?;
    Ok(page_buf)
}

#[instrument(level = "debug", skip(page_buf), err)]
pub fn parse_sqlite_schemas(page_buf: &[u8]) -> StorageResult<Vec<SqliteSchema>> {
    let page_header = page_header(page_buf, 0)?;
    let mut schemas: Vec<SqliteSchema> = vec![];

    for &ptr in page_header.cell_pointers() {
        let (cell, _) = parse_leaf_cell(&page_buf[(ptr as usize)..])?;
        schemas.push(parse(cell.record.values)?);
    }
    debug!(count = schemas.len(), "parsed sqlite schemas");

    Ok(schemas)
}

#[cfg(test)]
mod tests {
    use super::parse_query;
    use crate::{
        error::QueryError,
        sql::{Keyword, Token, tokenize},
    };

    fn parse(sql: &str) -> Result<super::ParsedTokens, QueryError> {
        parse_query(tokenize(sql)?)
    }

    #[test]
    fn errors_name_what_was_expected() {
        assert!(matches!(
            parse("select from t"),
            Err(QueryError::UnexpectedToken {
                found: Token::Keyword(Keyword::From),
                ..
            })
        ));
        assert!(matches!(
            parse("select a, from t"),
            Err(QueryError::UnexpectedToken {
                expected: "a column name",
                ..
            })
        ));
        assert!(matches!(
            parse("select a from"),
            Err(QueryError::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            parse("select a from t where c = 'x"),
            Err(QueryError::UnterminatedString { pos: 26 })
        ));
        assert!(parse("select a, b from t where c = 'x'").is_ok());
        assert!(parse("select count(*) from t").is_ok());
    }
}
