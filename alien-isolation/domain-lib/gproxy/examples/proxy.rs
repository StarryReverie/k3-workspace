extern crate alloc;
use std::{any::Any, fmt::Debug};

use gproxy::{no_check, proxy};
use spin::RwLock;
#[derive(Debug)]
pub enum AlienError {
    DOMAINCRASH,
}
type AlienResult<T> = Result<T, AlienError>;
pub trait Basic: Debug {
    fn is_active(&self) -> bool;
    fn domain_id(&self) -> u64;
}

pub trait DomainReload {
    fn reload(&self) -> AlienResult<()>;
}

pub trait DeviceBase {
    fn handle_irq(&self) -> AlienResult<()> {
        Err(AlienError::DOMAINCRASH)
    }
}

#[proxy(XXXDomainProxy, RwLock)]
pub trait XXXDomain: Basic + DeviceBase {
    fn init(&self) -> AlienResult<()>;
    #[no_check]
    fn xxxx(&self, x: usize) -> AlienResult<()>;
    #[no_check]
    fn yyy(&self) -> AlienResult<()>;
}

#[derive(Debug)]
pub struct DomainLoader {}

impl DomainLoader {}

gen_for_XXXDomain!();
pub enum FreeShared {
    Free,
    NotFree(u64),
}
pub fn free_domain_resource<T>(domain_id: u64, free_shared: FreeShared, free: T)
where
    T: Fn(*mut u8, usize),
{
}

fn yield_now() {}

#[no_mangle]
fn register_cont() {}

fn main() {
    // Once::new();
}
