use tracing::info;

use crate::{
    error::FormatError,
    helpers::{DbHeader, page_header},
};

pub fn run(page_buf: &[u8], db_hdr: DbHeader) -> Result<String, FormatError> {
    info!("executing db_info command");

    let page_header = page_header(page_buf, 0)?;

    let page_size = db_hdr.page_size();
    let cell_count = page_header.cell_count();

    let out = format!("database page size: {page_size}\nnumber of tables: {cell_count}");
    info!("finish");

    Ok(out)
}
