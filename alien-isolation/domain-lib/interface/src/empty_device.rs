use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::Basic;

#[proxy(EmptyDeviceDomainProxy, SRCU)]
pub trait EmptyDeviceDomain: Basic + DowncastSync {
    fn init(&self) -> AlienResult<()>;
    fn read(&self, data: DVec<u8>) -> AlienResult<DVec<u8>>;
    fn write(&self, data: &DVec<u8>) -> AlienResult<usize>;
}

impl_downcast!(sync EmptyDeviceDomain);
