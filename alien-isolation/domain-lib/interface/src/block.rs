use core::ops::Range;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(BlkDomainProxy,RwLock,Range<usize>)]
pub trait BlkDeviceDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, device_info: &Range<usize>) -> AlienResult<()>;
    fn read_block(&self, block: u32, data: DVec<u8>) -> AlienResult<DVec<u8>>;
    fn write_block(&self, block: u32, data: &DVec<u8>) -> AlienResult<usize>;
    fn get_capacity(&self) -> AlienResult<u64>;
    fn flush(&self) -> AlienResult<()>;
}

impl_downcast!(sync  BlkDeviceDomain);
