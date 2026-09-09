use std::os::unix::fs::FileExt;
use std::{fs::File, io::Read};

use tracing::info;

use crate::commands::helpers::{DbHeader, db_header, page_header, read_page};

pub(crate) fn run(file: &File, db_hdr: DbHeader) -> anyhow::Result<()> {
    info!("executing db_info command");
    let db_header = db_header(&file)?;
    let page_size = db_header.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0)?;

    let offset: usize = 0;

    let page_header = page_header(&page_buf, 0)?;
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    eprintln!("Logs from your program will appear here!");

    // TODO: Uncomment the code below to pass the first stage
    println!("database page size: {}", page_size);
    println!("number of tables: {}", page_header.cell_count());
    info!("finish");
    Ok(())
}
