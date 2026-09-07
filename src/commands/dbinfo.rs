use std::{fs::File, io::Read};
use std::os::unix::fs::FileExt;

use crate::commands::helpers::{DbHeader, db_header, read_page};



pub(crate) fn run(path: String) -> anyhow::Result<()> {
    let mut file = File::open(path)?;
    let db_header = db_header(&file)?;
    let page_size = db_header.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0);

    let offset: usize = 0;

    let cell_count = u16::from_be_bytes([])
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    eprintln!("Logs from your program will appear here!");

    // TODO: Uncomment the code below to pass the first stage
    println!("database page size: {}", page_size);
    println!("number of tables: {}", page_size);
    Ok(())
}
