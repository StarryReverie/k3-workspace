use alloc::boxed::Box;
use core::{any::Any, fmt::Debug};

use memory_addr::VirtAddr;

bitflags::bitflags! {
    /// Generic page table entry flags that indicate the corresponding mapped
    /// memory region permissions and attributes.
    #[derive(Debug,Copy, Clone)]
    pub struct DomainMappingFlags: usize {
        /// The memory is readable.
        const READ          = 1 << 0;
        /// The memory is writable.
        const WRITE         = 1 << 1;
        /// The memory is executable.
        const EXECUTE       = 1 << 2;
    }
}

pub trait DomainArea: Send + Sync + Debug + Any {
    fn as_slice(&self) -> &[u8];
    fn as_mut_slice(&self) -> &mut [u8];
    fn start_virtual_address(&self) -> VirtAddr;
    fn any(self: Box<Self>) -> Box<dyn Any>;
}

pub trait DomainVmOps {
    fn map_domain_area(size: usize) -> Box<dyn DomainArea>;
    fn unmap_domain_area(area: Box<dyn DomainArea>);
    fn set_memory_x(start: usize, pages: usize) -> Result<(), &'static str>;
}
