use core::ops::Range;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use pconst::io::RtcTime;
use shared_heap::DBox;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(RtcDomainProxy,RwLock,Range<usize>)]
pub trait RtcDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, device_info: &Range<usize>) -> AlienResult<()>;
    fn read_time(&self, time: DBox<RtcTime>) -> AlienResult<DBox<RtcTime>>;
}

impl_downcast!(sync RtcDomain);
