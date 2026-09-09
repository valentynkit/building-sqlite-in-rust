use std::os::unix::fs::FileExt;
use std::{fs::File, io::Read};

use tracing::info;

use crate::commands::helpers::{DbHeader, db_header, page_header, read_page};

pub(crate) fn run(page_buf: &[u8], db_hdr: DbHeader) -> anyhow::Result<String> {
    info!("executing db_info command");

    let page_header = page_header(&page_buf, 0)?;

    let page_size = db_hdr.page_size();
    let cell_count = page_header.cell_count();

    let out = format!("database page size: {page_size}\nnumber of tables: {cell_count}");
    info!("finish");

    Ok(out)
}
