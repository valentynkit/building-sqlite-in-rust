use std::fs::File;

use tracing::info;

use crate::commands::helpers::{DbHeader, page_header, read_page};

pub(crate) fn run(file: &File, db_hdr: DbHeader) -> anyhow::Result<()> {
    info!("executing .tables command");
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0)?;

    let page_header = page_header(page_buf, 0)?;
    for ptr in page_header.cell_pointers() {}

    // TODO: Uncomment the code below to pass the first stage
    println!("database page size: {}", page_size);
    println!("number of tables: {}", page_header.cell_count());
    info!("finish");
    Ok(())
}
