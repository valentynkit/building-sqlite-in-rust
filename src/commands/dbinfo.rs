use tracing::{info, instrument};

use crate::{error::FormatError, helpers::page_header};

#[instrument(level = "info", skip(page_buf), ret, err)]
pub fn run(page_buf: &[u8]) -> Result<String, FormatError> {
    info!("executing db_info command");

    let page_header = page_header(page_buf, 0)?;

    let cell_count = page_header.cell_count();

    let out = format!("number of tables: {cell_count}");
    info!("finish");

    Ok(out)
}
