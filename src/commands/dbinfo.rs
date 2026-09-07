use std::{fs::File, io::Read};

pub(crate) fn run(path: String) -> anyhow::Result<()> {
    let mut file = File::open(path)?;
    let mut header = [0; 100];
    file.read_exact(&mut header)?;

    // The page size is stored at the 16th byte offset, using 2 bytes in big-endian order
    #[allow(unused_variables)]
    let page_size = u16::from_be_bytes([header[16], header[17]]);

    // You can use print statements as follows for debugging, they'll be visible when running tests.
    eprintln!("Logs from your program will appear here!");

    // TODO: Uncomment the code below to pass the first stage
    println!("database page size: {}", page_size);
    Ok(())
}
