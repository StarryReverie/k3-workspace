use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(CacheBlkDomainProxy, RwLock, String)]
pub trait CacheBlkDeviceDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, blk_domain_name: &str) -> AlienResult<()>;
    fn read(&self, offset: u64, buf: DVec<u8>) -> AlienResult<DVec<u8>>;
    fn write(&self, offset: u64, buf: &DVec<u8>) -> AlienResult<usize>;
    fn get_capacity(&self) -> AlienResult<u64>;
    fn flush(&self) -> AlienResult<()>;
}

impl_downcast!(sync CacheBlkDeviceDomain);
