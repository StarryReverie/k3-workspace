use core::ops::{Deref, DerefMut, Range};

use config::FRAME_SIZE;
use io::frame::BasicFrame;
use shared_heap::domain_id;

#[derive(Debug)]
pub struct FrameTracker(BasicFrame);

impl Deref for FrameTracker {
    type Target = BasicFrame;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FrameTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FrameTracker {
    /// Allocate `page_count` pages and return a `FrameTracker` pointing to the start of the allocated memory.
    pub fn new(page_count: usize) -> Self {
        let ptr = corelib::alloc_raw_pages(page_count, domain_id()) as usize;
        Self(BasicFrame {
            ptr,
            page_count,
            dealloc: true,
        })
    }
    pub fn create_trampoline() -> Self {
        let trampoline_phy_addr = corelib::trampoline_addr();
        Self(BasicFrame {
            ptr: trampoline_phy_addr,
            page_count: 1,
            dealloc: false,
        })
    }
    pub fn from_phy_range(r: Range<usize>) -> Self {
        assert_eq!(r.start % FRAME_SIZE, 0);
        assert_eq!(r.end % FRAME_SIZE, 0);
        Self(BasicFrame {
            ptr: r.start,
            page_count: (r.end - r.start) / FRAME_SIZE,
            dealloc: false,
        })
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        if self.dealloc {
            corelib::free_raw_pages(self.ptr as *mut u8, self.page_count, domain_id());
        }
    }
}
