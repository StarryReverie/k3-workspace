use core::ops::Range;

use memory_addr::{PhysAddr, VirtAddr};

#[derive(Debug, Clone)]
pub struct SafeIORegion {
    range: Range<PhysAddr>,
}

impl From<Range<usize>> for SafeIORegion {
    fn from(value: Range<usize>) -> Self {
        let start = PhysAddr::from(value.start);
        let end = PhysAddr::from(value.end);
        Self { range: start..end }
    }
}

impl SafeIORegion {
    pub fn new(range: Range<PhysAddr>) -> Self {
        Self { range }
    }

    pub fn as_bytes(&self) -> &[u8] {
        let start = self.range.start.as_usize();
        unsafe { core::slice::from_raw_parts(start as *const u8, self.size()) }
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        let start = self.range.start.as_usize();
        unsafe { core::slice::from_raw_parts_mut(start as *mut u8, self.size()) }
    }

    pub fn read_at<T: Copy>(&self, offset: usize) -> Result<T, ()> {
        if offset + core::mem::size_of::<T>() > self.size() {
            return Err(());
        }
        let start = self.range.start.as_usize();
        let ptr = (start + offset) as *const T;
        unsafe { Ok(ptr.read_volatile()) }
    }

    pub fn write_at<T: Copy>(&self, offset: usize, value: T) -> Result<(), ()> {
        if offset + core::mem::size_of::<T>() > self.size() {
            return Err(());
        }
        let start = self.range.start.as_usize();
        let ptr = (start + offset) as *mut T;
        unsafe { ptr.write_volatile(value) }
        Ok(())
    }

    pub fn phys_addr(&self) -> PhysAddr {
        self.range.start
    }

    pub fn phys_addr_range(&self) -> Range<PhysAddr> {
        self.range.clone()
    }

    pub fn virt_addr(&self) -> VirtAddr {
        VirtAddr::from(self.range.start.as_usize())
    }

    pub fn size(&self) -> usize {
        self.range.end.as_usize() - self.range.start.as_usize()
    }
}
