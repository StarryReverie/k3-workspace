use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DBox;
use task_meta::TaskSchedulingInfo;

use super::AlienResult;
use crate::Basic;

#[proxy(SchedulerDomainProxy, RwLock)]
pub trait SchedulerDomain: Basic + DowncastSync {
    fn init(&self) -> AlienResult<()>;
    /// add one task to scheduler
    fn add_task(&self, scheduling_info: DBox<TaskSchedulingInfo>) -> AlienResult<()>;
    /// The next task to run
    fn fetch_task(&self, info: DBox<TaskSchedulingInfo>) -> AlienResult<DBox<TaskSchedulingInfo>>;
}

impl_downcast!(sync SchedulerDomain);
