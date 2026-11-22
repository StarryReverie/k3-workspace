use core::{
    ops::{Deref, DerefMut},
    sync::atomic::AtomicUsize,
};

use memory_addr::{PhysAddr, VirtAddr};

#[derive(Debug)]
pub struct BasicFrame {
    pub ptr: usize,
    pub page_count: usize,
    // should be deallocated
    pub dealloc: bool,
}

impl BasicFrame {
    /// Return the physical address of the start of the frame.
    pub fn start_phy_addr(&self) -> PhysAddr {
        PhysAddr::from(self.ptr)
    }

    /// Return the virtual address of the start of the frame.
    pub fn start_virt_addr(&self) -> VirtAddr {
        VirtAddr::from(self.ptr)
    }

    /// Return the physical address of the end of the frame.
    pub fn end_phy_addr(&self) -> PhysAddr {
        PhysAddr::from(self.end())
    }

    /// Return the virtual address of the end of the frame.
    pub fn end_virt_addr(&self) -> VirtAddr {
        VirtAddr::from(self.end())
    }

    fn end(&self) -> usize {
        self.ptr + self.size()
    }

    pub fn size(&self) -> usize {
        self.page_count * 4096
    }

    pub fn clear(&self) {
        unsafe {
            core::ptr::write_bytes(self.ptr as *mut u8, 0, self.size());
        }
    }
    pub fn as_mut_slice_with<'a, T>(&self, offset: usize) -> &'a mut [T] {
        let t_size = core::mem::size_of::<T>();
        assert_eq!((self.size() - offset) % t_size, 0);
        let ptr = self.ptr + offset;
        unsafe { core::slice::from_raw_parts_mut(ptr as *mut T, (self.size() - offset) / t_size) }
    }
    pub fn as_slice_with<'a, T>(&self, offset: usize) -> &'a [T] {
        let t_size = core::mem::size_of::<T>();
        assert_eq!((self.size() - offset) % t_size, 0);
        let ptr = self.ptr + offset;
        unsafe { core::slice::from_raw_parts(ptr as *const T, (self.size() - offset) / t_size) }
    }

    pub fn as_mut_with<'a, T: Sized>(&self, offset: usize) -> &'a mut T {
        assert!(offset + core::mem::size_of::<T>() <= self.size());
        let ptr = self.ptr + offset;
        unsafe { &mut *(ptr as *mut T) }
    }

    pub fn as_with<'a, T: Sized>(&self, offset: usize) -> &'a T {
        assert!(offset + core::mem::size_of::<T>() <= self.size());
        let ptr = self.ptr + offset;
        unsafe { &*(ptr as *const T) }
    }

    pub fn read_value_atomic(&self, offset: usize) -> usize {
        assert!(offset + core::mem::size_of::<usize>() <= self.size());
        let ptr = self.ptr + offset;
        unsafe {
            AtomicUsize::from_ptr(ptr as *mut usize).load(core::sync::atomic::Ordering::SeqCst)
        }
    }

    pub fn write_value_atomic(&self, offset: usize, value: usize) {
        assert!(offset + core::mem::size_of::<usize>() <= self.size());
        let ptr = self.ptr + offset;
        unsafe {
            AtomicUsize::from_ptr(ptr as *mut usize)
                .store(value, core::sync::atomic::Ordering::SeqCst)
        }
    }
}

impl Deref for BasicFrame {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { core::slice::from_raw_parts(self.ptr as *const u8, self.size()) }
    }
}

impl DerefMut for BasicFrame {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { core::slice::from_raw_parts_mut(self.ptr as *mut u8, self.size()) }
    }
}
