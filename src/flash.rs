use core::fmt::Debug;

#[cfg(feature = "std")]
use crate::config::*;
pub use crate::types::PageID;

/// NOR flash memory trait
/// TODO use embedded-storage
pub trait Flash {
    type Error: Debug;

    fn page_count(&self) -> usize;
    async fn erase(&mut self, page_id: PageID) -> Result<(), Self::Error>;
    async fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) -> Result<(), Self::Error>;
    async fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) -> Result<(), Self::Error>;
}

impl<T: Flash> Flash for &mut T {
    type Error = T::Error;
    fn page_count(&self) -> usize {
        T::page_count(self)
    }
    async fn erase(&mut self, page_id: PageID) -> Result<(), Self::Error> {
        T::erase(self, page_id).await
    }
    async fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) -> Result<(), Self::Error> {
        T::read(self, page_id, offset, data).await
    }
    async fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) -> Result<(), Self::Error> {
        T::write(self, page_id, offset, data).await
    }
}

/// Fake in-memory flash
#[cfg(feature = "std")]
pub struct MemFlash {
    pub data: Vec<u8>,
    pub read_count: usize,
    pub read_bytes: usize,
    pub write_count: usize,
    pub write_bytes: usize,
    pub erase_count: usize,
    pub erase_bytes: usize,
}

#[cfg(feature = "std")]
impl MemFlash {
    pub fn new() -> Self {
        Self {
            data: vec![ERASE_VALUE; PAGE_SIZE * MAX_PAGE_COUNT],
            read_count: 0,
            read_bytes: 0,
            write_count: 0,
            write_bytes: 0,
            erase_count: 0,
            erase_bytes: 0,
        }
    }

    pub fn reset_counters(&mut self) {
        self.read_count = 0;
        self.read_bytes = 0;
        self.write_count = 0;
        self.write_bytes = 0;
        self.erase_count = 0;
        self.erase_bytes = 0;
    }
}

#[cfg(feature = "std")]
impl Flash for MemFlash {
    type Error = core::convert::Infallible;

    fn page_count(&self) -> usize {
        MAX_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), Self::Error> {
        let page_id = page_id.index();

        assert!(page_id < self.page_count());
        self.data[page_id * PAGE_SIZE..][..PAGE_SIZE].fill(ERASE_VALUE);
        self.erase_count += 1;
        self.erase_bytes += PAGE_SIZE;

        Ok(())
    }

    async fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) -> Result<(), Self::Error> {
        let page_id = page_id.index();

        assert!(page_id < self.page_count());
        assert!(offset <= PAGE_SIZE);
        assert!(offset + data.len() <= PAGE_SIZE);
        assert!(offset % ALIGN == 0);
        assert!(data.len() % ALIGN == 0);

        let mem = &self.data[page_id * PAGE_SIZE + offset..][..data.len()];
        data.copy_from_slice(mem);
        self.read_count += 1;
        self.read_bytes += data.len();

        Ok(())
    }

    async fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) -> Result<(), Self::Error> {
        let page_id = page_id.index();

        assert!(page_id < self.page_count());
        assert!(offset <= PAGE_SIZE);
        assert!(offset + data.len() <= PAGE_SIZE);
        assert!(offset % ALIGN == 0);
        assert!(data.len() % ALIGN == 0);

        let mem = &mut self.data[page_id * PAGE_SIZE + offset..][..data.len()];
        assert!(mem.iter().all(|x| *x == ERASE_VALUE));
        mem.copy_from_slice(data);
        self.write_count += 1;
        self.write_bytes += data.len();

        Ok(())
    }
}
