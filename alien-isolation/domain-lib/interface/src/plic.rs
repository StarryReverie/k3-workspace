use core::ops::Range;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::Basic;
#[proxy(PLICDomainProxy, RwLock, PlicInfo)]
pub trait PLICDomain: Basic + DowncastSync {
    fn init(&self, plic_info: &PlicInfo) -> AlienResult<()>;
    fn handle_irq(&self) -> AlienResult<()>;
    fn register_irq(&self, irq: usize, device_domain_name: &DVec<u8>) -> AlienResult<()>;
    fn irq_info(&self, buf: DVec<u8>) -> AlienResult<DVec<u8>>;
}

impl_downcast!(sync PLICDomain);

#[derive(Clone, Debug)]
pub struct PlicInfo {
    pub device_info: Range<usize>,
    pub ty: PlicType,
}

#[derive(Copy, Clone, Debug)]
pub enum PlicType {
    Qemu,
    SiFive,
}
