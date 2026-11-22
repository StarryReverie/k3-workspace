#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

use alloc::boxed::Box;
use core::fmt::Debug;

use basic::AlienResult;
use interface::{define_unwind_for_EmptyDeviceDomain, Basic, EmptyDeviceDomain};
use shared_heap::DVec;

#[derive(Debug)]
pub struct RandomDeviceDomainImpl;

impl Basic for RandomDeviceDomainImpl {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl EmptyDeviceDomain for RandomDeviceDomainImpl {
    fn init(&self) -> AlienResult<()> {
        Ok(())
    }

    fn read(&self, mut data: DVec<u8>) -> AlienResult<DVec<u8>> {
        let mut current_time = basic::time::read_time_ms();
        data.as_mut_slice().iter_mut().for_each(|x| {
            *x = current_time as u8;
            current_time = current_time.wrapping_sub(1);
        });
        Ok(data)
    }
    fn write(&self, data: &DVec<u8>) -> AlienResult<usize> {
        Ok(data.len())
    }
}

define_unwind_for_EmptyDeviceDomain!(RandomDeviceDomainImpl);

pub fn main() -> Box<dyn EmptyDeviceDomain> {
    Box::new(UnwindWrap::new(RandomDeviceDomainImpl))
}
