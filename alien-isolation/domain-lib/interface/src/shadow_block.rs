use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::{no_check, proxy};
use shared_heap::DVec;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(ShadowBlockDomainProxy, SRCU, String)]
pub trait ShadowBlockDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, blk_domain: &str) -> AlienResult<()>;
    #[no_check]
    fn read_block(&self, block: u32, data: DVec<u8>) -> AlienResult<DVec<u8>>;
    #[no_check]
    fn write_block(&self, block: u32, data: &DVec<u8>) -> AlienResult<usize>;
    #[no_check]
    fn get_capacity(&self) -> AlienResult<u64>;
    #[no_check]
    fn flush(&self) -> AlienResult<()>;
}

impl_downcast!(sync  ShadowBlockDomain);
