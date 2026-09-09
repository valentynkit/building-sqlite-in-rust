use std::fs::File;

use anyhow::bail;
use tracing::{debug, info};

use crate::commands::helpers::{DbHeader, SqliteSchema, page_header, parse_cell, read_page};

pub(crate) fn run(
    file: &File,
    schemas: Vec<SqliteSchema>,
    db_hdr: DbHeader,
    query: Vec<String>,
) -> anyhow::Result<String> {
    assert_eq!(query.len(), 1, "query should be one element");
    info!("executing command {query:?}, query_len {}", query.len());
    let tokens = query[0].split(' ').collect::<Vec<&str>>();
    let second_arg = tokens
        .get(1)
        .expect("query is expected to have second element");

    let from_table = tokens
        .last()
        .expect("query should have the last item after FROM available");

    let Some(item) = schemas.iter().find(|&item| item.tbl_name() == *from_table) else {
        bail!("could find table with tbl_name: {from_table}");
    };

    debug!(?item);

    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, item.rootpage_index())?;

    let page_header = page_header(&page_buf, item.rootpage_index())?;

    for &ptr in page_header.cell_pointers() {
        debug!(offset = ptr, "start parsing cell");
        let (_cell, _) = parse_cell(&page_buf[(ptr as usize)..])?;
    }
    let out = if second_arg.eq_ignore_ascii_case("count(*)") {
        format!("{}", page_header.cell_count())
    } else {
        "Uknown second argument".into()
    };

    Ok(out)
}
