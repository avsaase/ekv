use crate::config::*;
pub use crate::types::PageID;

/// NOR flash memory trait
/// TODO use embedded-storage
pub trait Flash {
    fn erase(&mut self, page_id: PageID);
    fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]);
    fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]);
}

impl<T: Flash> Flash for &mut T {
    fn erase(&mut self, page_id: PageID) {
        T::erase(self, page_id)
    }
    fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) {
        T::read(self, page_id, offset, data)
    }
    fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) {
        T::write(self, page_id, offset, data)
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
            data: vec![ERASE_VALUE; PAGE_SIZE * PAGE_COUNT],
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
    fn erase(&mut self, page_id: PageID) {
        let page_id = page_id.index();

        assert!(page_id < PAGE_COUNT);
        self.data[page_id * PAGE_SIZE..][..PAGE_SIZE].fill(ERASE_VALUE);
        self.erase_count += 1;
        self.erase_bytes += PAGE_SIZE;
    }

    fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) {
        let page_id = page_id.index();

        assert!(page_id < PAGE_COUNT);
        assert!(offset <= PAGE_SIZE);
        assert!(offset + data.len() <= PAGE_SIZE);
        assert!(offset % WRITE_SIZE == 0);
        assert!(data.len() % WRITE_SIZE == 0);

        let mem = &self.data[page_id * PAGE_SIZE + offset..][..data.len()];
        data.copy_from_slice(mem);
        self.read_count += 1;
        self.read_bytes += data.len();
    }

    fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) {
        let page_id = page_id.index();

        assert!(page_id < PAGE_COUNT);
        assert!(offset <= PAGE_SIZE);
        assert!(offset + data.len() <= PAGE_SIZE);
        assert!(offset % WRITE_SIZE == 0);
        assert!(data.len() % WRITE_SIZE == 0);

        let mem = &mut self.data[page_id * PAGE_SIZE + offset..][..data.len()];
        assert!(mem.iter().all(|x| *x == ERASE_VALUE));
        mem.copy_from_slice(data);
        self.write_count += 1;
        self.write_bytes += data.len();
    }
}
