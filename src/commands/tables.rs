use std::fs::File;

use tracing::{debug, info};

use crate::commands::helpers::{Column, DbHeader, page_header, parse_cell, read_page};

pub(crate) fn run(file: &File, db_hdr: DbHeader) -> anyhow::Result<()> {
    info!("executing .tables command");
    let page_size = db_hdr.page_size();
    let mut page_buf = vec![0u8; page_size as usize];
    read_page(&file, &mut page_buf, page_size, 0)?;

    let page_header = page_header(&page_buf, 0)?;
    let mut table_names: Vec<String> = vec![];
    for &ptr in page_header.cell_pointers() {
        debug!(offset = ptr, "start parsing cell");
        let (cell, _) = parse_cell(&mut page_buf[(ptr as usize)..])?;
        let Some(Column::Text(table_name)) = cell.record.values.get(2) else {
            return Err(anyhow::anyhow!("record at index 2, doesn't contain text"));
        };

        table_names.push(table_name.into());
    }

    // TODO: Uncomment the code below to pass the first stage
    let out = table_names.join(" ");
    println!("table names: {}", out);
    info!("finish");
    Ok(())
}
