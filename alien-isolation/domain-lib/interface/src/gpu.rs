use core::ops::Range;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(GpuDomainProxy,RwLock,Range<usize>)]
pub trait GpuDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, device_info: &Range<usize>) -> AlienResult<()>;
    fn flush(&self) -> AlienResult<()>;
    fn fill(&self, offset: u32, buf: &DVec<u8>) -> AlienResult<usize>;
    fn buffer_range(&self) -> AlienResult<Range<usize>>;
}

impl_downcast!(sync GpuDomain);
