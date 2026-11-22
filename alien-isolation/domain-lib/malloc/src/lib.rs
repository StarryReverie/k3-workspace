#![no_std]
use core::alloc::GlobalAlloc;

use buddy_system_allocator::LockedHeap;
use shared_heap::domain_id;

pub struct HeapAllocator {
    allocator: LockedHeap<32>,
    alloc_pages: fn(n: usize, domain_id: u64) -> *mut u8,
}

impl HeapAllocator {
    pub const fn new(alloc_pages: fn(n: usize, domain_id: u64) -> *mut u8) -> Self {
        Self {
            allocator: LockedHeap::<32>::new(),
            alloc_pages,
        }
    }
}

unsafe impl GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let ptr = self.allocator.alloc(layout);
        if ptr.is_null() {
            let need_pages = (layout.size() + 4096 - 1) / 4096;
            let need_pages = (need_pages * 2).next_power_of_two();
            // we alloc two times of the pages we need
            let f = self.alloc_pages;
            let new_pages = f(need_pages, domain_id());
            assert!(!new_pages.is_null());
            self.allocator
                .lock()
                .add_to_heap(new_pages as usize, need_pages * 4096 + new_pages as usize);
            let ptr = self.allocator.alloc(layout);
            assert!(
                !ptr.is_null(),
                "need pages: {}, layout:{:#x?}",
                need_pages,
                layout
            );
            ptr
        } else {
            ptr
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        self.allocator.dealloc(ptr, layout);
    }
}
