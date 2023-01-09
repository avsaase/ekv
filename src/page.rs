use std::marker::PhantomData;

use crate::config::*;
use crate::file::Header;
use crate::flash::Flash;
use crate::Error;

const PAGE_HEADER_MAGIC: u32 = 0xc4e21c75;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct PageHeader {
    magic: u32,
    // Higher layer data
    file_header: Header,
}
impl_bytes!(PageHeader);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct ChunkHeader {
    len: u32,
}
impl_bytes!(ChunkHeader);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadError {
    Eof,
    Corrupted,
}

impl From<Error> for ReadError {
    fn from(err: Error) -> Self {
        match err {
            Error::Corrupted => Self::Corrupted,
        }
    }
}

pub struct PageManager<F: Flash> {
    _phantom: PhantomData<F>,
}

impl<F: Flash> PageManager<F> {
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }

    fn write_page_header(flash: &mut F, page_id: PageID, h: PageHeader) {
        let buf = h.to_bytes();
        flash.write(page_id as _, 0, &buf);
    }

    fn read_page_header(flash: &mut F, page_id: PageID) -> Result<PageHeader, Error> {
        let mut buf = [0u8; PageHeader::SIZE];
        flash.read(page_id as _, 0, &mut buf);
        let h = PageHeader::from_bytes(buf);
        if h.magic != PAGE_HEADER_MAGIC {
            return Err(Error::Corrupted);
        }

        Ok(h)
    }

    pub fn read_header(&mut self, flash: &mut F, page_id: PageID) -> Result<Header, Error> {
        let h = Self::read_page_header(flash, page_id)?;
        Ok(h.file_header)
    }

    pub fn read(&mut self, flash: &mut F, page_id: PageID) -> Result<(Header, PageReader), Error> {
        trace!("page: read {:?}", page_id);
        let header = Self::read_page_header(flash, page_id)?;
        let mut r = PageReader {
            page_id,
            total_pos: 0,
            at_end: false,
            chunk_offset: PageHeader::SIZE,
            chunk_len: 0,
            chunk_pos: 0,
        };
        r.open_chunk(flash)?;
        Ok((header.file_header, r))
    }

    pub fn write(&mut self, _flash: &mut F, page_id: PageID) -> PageWriter {
        trace!("page: write {:?}", page_id);
        PageWriter {
            page_id,
            needs_erase: true,
            total_pos: 0,
            chunk_offset: PageHeader::SIZE,
            chunk_pos: 0,
        }
    }
}

pub struct PageReader {
    page_id: PageID,

    /// Total pos within the page (all chunks)
    total_pos: usize,

    /// true if we've reached the end of the page.
    at_end: bool,

    /// Offset where the chunk we're currently writing starts.
    chunk_offset: usize,
    /// pos within the current chunk.
    chunk_pos: usize,
    /// Data bytes in the chunk we're currently writing.
    chunk_len: usize,
}

impl PageReader {
    pub fn page_id(&self) -> PageID {
        self.page_id
    }

    fn next_chunk(&mut self, flash: &mut impl Flash) -> Result<bool, Error> {
        self.chunk_offset += ChunkHeader::SIZE + self.chunk_len;
        self.open_chunk(flash)
    }

    fn open_chunk(&mut self, flash: &mut impl Flash) -> Result<bool, Error> {
        let data_start = self.chunk_offset + ChunkHeader::SIZE;
        if data_start > PAGE_SIZE {
            self.at_end = true;
            return Ok(false);
        }

        let mut header = [0u8; ChunkHeader::SIZE];
        flash.read(self.page_id as _, self.chunk_offset, &mut header);
        let header = ChunkHeader::from_bytes(header);

        // TODO make this more robust against written but not committed garbage.
        if header.len == 0xFFFF_FFFF {
            self.at_end = true;
            return Ok(false);
        }

        let data_end = data_start.checked_add(header.len as usize).ok_or(Error::Corrupted)?;
        if data_end > PAGE_SIZE {
            return Err(Error::Corrupted);
        }

        trace!("open chunk at offs={} len={}", self.chunk_offset, header.len);

        self.chunk_pos = 0;
        self.chunk_len = header.len as usize;

        Ok(true)
    }

    pub fn read(&mut self, flash: &mut impl Flash, data: &mut [u8]) -> Result<usize, Error> {
        trace!("PageReader({:?}): read({})", self.page_id, data.len());
        if self.at_end || data.len() == 0 {
            trace!("read: at end or zero len");
            return Ok(0);
        }

        if self.chunk_pos == self.chunk_len {
            trace!("read: at end of chunk");
            if !self.next_chunk(flash)? {
                trace!("read: no next chunk, we're at end.");
                return Ok(0);
            }
        }

        let n = data.len().min(self.chunk_len - self.chunk_pos);
        let offset = self.chunk_offset + ChunkHeader::SIZE + self.chunk_pos;
        flash.read(self.page_id as _, offset, &mut data[..n]);
        self.total_pos += n;
        self.chunk_pos += n;
        trace!("read: done, n={}", n);
        Ok(n)
    }

    pub fn skip(&mut self, flash: &mut impl Flash, len: usize) -> Result<usize, Error> {
        trace!("PageReader({:?}): skip({})", self.page_id, len);
        if self.at_end || len == 0 {
            trace!("skip: at end or zero len");
            return Ok(0);
        }

        if self.chunk_pos == self.chunk_len {
            trace!("skip: at end of chunk");
            if !self.next_chunk(flash)? {
                trace!("skip: no next chunk, we're at end.");
                return Ok(0);
            }
        }

        let n = len.min(self.chunk_len - self.chunk_pos);
        self.total_pos += n;
        self.chunk_pos += n;
        trace!("skip: done, n={}", n);
        Ok(n)
    }

    pub fn is_at_eof(&mut self, flash: &mut impl Flash) -> Result<bool, Error> {
        if self.at_end {
            return Ok(true);
        }
        if self.chunk_pos == self.chunk_len {
            if !self.next_chunk(flash)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub struct PageWriter {
    page_id: PageID,
    needs_erase: bool,

    /// Total data bytes in page (all chunks)
    total_pos: usize,

    /// Offset where the chunk we're currently writing starts.
    chunk_offset: usize,
    /// Data bytes in the chunk we're currently writing.
    chunk_pos: usize,
}

impl PageWriter {
    pub fn len(&self) -> usize {
        self.total_pos
    }

    pub fn page_id(&self) -> PageID {
        self.page_id
    }

    pub fn write(&mut self, flash: &mut impl Flash, data: &[u8]) -> usize {
        let offset = self.chunk_offset + ChunkHeader::SIZE + self.chunk_pos;
        let max_write = PAGE_SIZE.saturating_sub(offset);
        let n = data.len().min(max_write);
        if n == 0 {
            return 0;
        }

        self.erase_if_needed(flash);

        flash.write(self.page_id as _, offset, &data[..n]);
        self.total_pos += n;
        self.chunk_pos += n;
        n
    }

    fn erase_if_needed(&mut self, flash: &mut impl Flash) {
        if self.needs_erase {
            flash.erase(self.page_id as _);
            self.needs_erase = false;
        }
    }

    pub fn write_header(&mut self, flash: &mut impl Flash, header: Header) {
        self.erase_if_needed(flash);

        PageManager::write_page_header(
            flash,
            self.page_id,
            PageHeader {
                magic: PAGE_HEADER_MAGIC,
                file_header: header,
            },
        );
    }

    pub fn commit(&mut self, flash: &mut impl Flash) {
        if self.chunk_pos == 0 {
            // nothing to commit.
            return;
        }

        self.erase_if_needed(flash);

        let h = ChunkHeader {
            len: self.chunk_pos as u32,
        };
        flash.write(self.page_id as _, self.chunk_offset, &h.to_bytes());

        // Prepare for next chunk.
        self.chunk_offset += ChunkHeader::SIZE + self.chunk_pos;
        self.chunk_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use super::*;
    use crate::flash::MemFlash;

    #[test]
    fn test_header() {
        let f = &mut MemFlash::new();

        let h = PageHeader {
            magic: PAGE_HEADER_MAGIC,
            file_header: Header::DUMMY,
        };
        PageManager::write_page_header(f, 0, h);
        let h2 = PageManager::read_page_header(f, 0).unwrap();
        assert_eq!(h, h2)
    }

    #[test]
    fn test_header_read_unwritten() {
        let f = &mut MemFlash::new();

        let res = PageManager::read_page_header(f, 0);
        assert!(matches!(res, Err(Error::Corrupted)))
    }

    #[test]
    fn test_read_unwritten() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        // Read
        let res = b.read(f, 0);
        assert!(matches!(res, Err(Error::Corrupted)));
    }

    #[test]
    fn test_read_uncommitted() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        let data = dummy_data(13);

        // Write
        let mut w = b.write(f, 0);
        w.write(f, &data);
        drop(w); // don't commit

        // Read
        let res = b.read(f, 0);
        assert!(matches!(res, Err(Error::Corrupted)));
    }

    #[test]
    fn test_write_short() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        let data = dummy_data(13);

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &data);
        assert_eq!(n, 13);
        w.write_header(f, Header::DUMMY);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; data.len()];
        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 13);
        assert_eq!(data, buf);

        // Remount
        let mut b = PageManager::new();

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; data.len()];
        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 13);
        assert_eq!(data, buf);
    }

    #[test]
    fn test_overread() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        let data = dummy_data(13);

        // Write
        let mut w = b.write(f, 0);
        w.write(f, &data);
        w.write_header(f, Header::DUMMY);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; 1024];
        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 13);
        assert_eq!(data, buf[..13]);
    }

    #[test]
    fn test_overwrite() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        let data = dummy_data(65536);

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &data);
        assert_eq!(n, PAGE_MAX_PAYLOAD_SIZE);
        w.write_header(f, Header::DUMMY);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; PAGE_MAX_PAYLOAD_SIZE];
        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, PAGE_MAX_PAYLOAD_SIZE);
        assert_eq!(data[..13], buf[..13]);
    }

    #[test]
    fn test_write_many() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &[1, 2, 3]);
        assert_eq!(n, 3);
        let n = w.write(f, &[4, 5, 6, 7, 8, 9]);
        assert_eq!(n, 6);
        w.write_header(f, Header::DUMMY);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; PAGE_MAX_PAYLOAD_SIZE];
        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 9);
        assert_eq!(buf[..9], [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_read_many() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(n, 9);
        w.write_header(f, Header::DUMMY);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; PAGE_MAX_PAYLOAD_SIZE];

        let n = r.read(f, &mut buf[..3]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(buf[..3], [1, 2, 3]);

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 6);
        assert_eq!(buf[..6], [4, 5, 6, 7, 8, 9]);

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_multichunk() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(n, 9);
        w.write_header(f, Header::DUMMY);
        w.commit(f);
        let n = w.write(f, &[10, 11, 12]);
        assert_eq!(n, 3);
        w.commit(f);

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; PAGE_MAX_PAYLOAD_SIZE];

        let n = r.read(f, &mut buf[..3]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(buf[..3], [1, 2, 3]);

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 6);
        assert_eq!(buf[..6], [4, 5, 6, 7, 8, 9]);

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 3);
        assert_eq!(buf[..3], [10, 11, 12]);
    }

    #[test]
    fn test_multichunk_no_commit() {
        let f = &mut MemFlash::new();
        let mut b = PageManager::new();

        // Write
        let mut w = b.write(f, 0);
        let n = w.write(f, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(n, 9);
        w.write_header(f, Header::DUMMY);
        w.commit(f);
        let n = w.write(f, &[10, 11, 12]);
        assert_eq!(n, 3);
        // no commit!

        // Read
        let (h, mut r) = b.read(f, 0).unwrap();
        assert_eq!(h, Header::DUMMY);
        let mut buf = vec![0; PAGE_MAX_PAYLOAD_SIZE];

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 9);
        assert_eq!(buf[..9], [1, 2, 3, 4, 5, 6, 7, 8, 9]);

        let n = r.read(f, &mut buf).unwrap();
        assert_eq!(n, 0);
    }

    fn dummy_data(len: usize) -> Vec<u8> {
        let mut res = vec![0; len];
        for (i, v) in res.iter_mut().enumerate() {
            *v = i as u8 ^ (i >> 8) as u8 ^ (i >> 16) as u8 ^ (i >> 24) as u8;
        }
        res
    }
}
