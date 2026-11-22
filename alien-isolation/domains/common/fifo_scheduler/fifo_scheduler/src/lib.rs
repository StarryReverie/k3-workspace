#![feature(allocator_api)]
#![no_std]
#![forbid(unsafe_code)]

mod scheduler;
extern crate alloc;

use alloc::boxed::Box;

use common_scheduler::{CommonSchedulerDomain, UnwindWrap};
use interface::SchedulerDomain;

use crate::scheduler::CustomFiFoScheduler;

pub fn main() -> Box<dyn SchedulerDomain> {
    basic::println!("Fifo Scheduler");
    Box::new(UnwindWrap::new(CommonSchedulerDomain::new(Box::new(
        CustomFiFoScheduler::new(),
    ))))
}
