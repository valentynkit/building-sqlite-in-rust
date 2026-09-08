use crate::constants::{DB_HEADER_SIZE, PAGE_SIZE};
use std::fs::File;
use std::os::unix::fs::FileExt;

pub(crate) struct DbHeader {
    page_size: u16,
}

impl DbHeader {
    pub(crate) fn new(page_size: u16) -> DbHeader {
        DbHeader { page_size }
    }
    pub(crate) fn page_size(&self) -> u16 {
        self.page_size
    }
}

pub(crate) struct PageHeader {
    page_type: PageType,
    cell_count: u16,
}

impl PageHeader {
    pub(crate) fn new(page_type: PageType, cell_count: u16) -> PageHeader {
        PageHeader {
            page_type,
            cell_count,
        }
    }
    pub(crate) fn cell_count(&self) -> u16 {
        self.cell_count
    }
}
pub(crate) enum PageType {
    InteriorIndex,
    InteriorTable,
    LeafIndex,
    LeafTable,
}

impl TryFrom<u8> for PageType {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(PageType::InteriorIndex),
            0x05 => Ok(PageType::InteriorTable),
            0x0a => Ok(PageType::LeafIndex),
            0x0d => Ok(PageType::LeafTable),
            b => anyhow::bail!("invalid page type: {b:#x}"),
        }
    }
}
pub(crate) fn db_header(file: &File) -> anyhow::Result<DbHeader> {
    let mut db_header: [u8; 100] = [0; 100];
    file.read_exact_at(&mut db_header, 0)?;

    let page_size = u16::from_be_bytes([db_header[16], db_header[17]]);
    Ok(DbHeader::new(page_size))
}

pub(crate) fn page_header(page: Vec<u8>, num_page: usize) -> anyhow::Result<PageHeader> {
    let mut offset: usize = 0;
    let mut out: [u8; 12] = [0; 12];
    if num_page == 1 {
        offset += DB_HEADER_SIZE;
    }
    let page_type = PageType::try_from(page[offset])?;

    let _len: usize = match page_type {
        PageType::LeafIndex | PageType::LeafTable => 8,
        PageType::InteriorIndex | PageType::InteriorTable => 12,
    };

    let cell_count = u16::from_be_bytes([page[offset + 3], page[offset + 4]]);
    Ok(PageHeader::new(page_type, cell_count))
}

pub(crate) fn read_page(
    file: &File,
    buf: &mut [u8],
    page_size: u16,
    page_num: usize,
) -> anyhow::Result<()> {
    let offset = page_size as usize * page_num;
    file.read_exact_at(buf, offset as u64)?;
    Ok(())
}
