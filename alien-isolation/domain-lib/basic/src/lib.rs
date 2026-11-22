#![feature(downcast_unchecked)]
#![no_std]
#[macro_use]
pub mod console;
pub mod arch;
pub mod bus;
pub mod config;
pub mod io;
#[cfg(feature = "log")]
pub mod logging;
pub mod sync;
#[cfg(feature = "task")]
pub mod task;
pub mod time;
pub mod vm;

extern crate alloc;

use alloc::{boxed::Box, format, sync::Arc};

use corelib::domain_info::DomainInfo;
pub use corelib::{
    add_one_task, backtrace, blk_crash_trick, checkout_shared_data, constants, create_domain,
    current_tid, exit_now, get_domain, get_task_priority, is_task_exit, kernel_satp,
    register_domain, reload_domain, remove_task, set_task_priority, trap_from_user, trap_to_user,
    update_domain, vaddr_to_paddr_in_kernel, wait_now, wake_up_wait_task, write_console, yield_now,
    AlienError, AlienResult, CoreFunction,
};
pub use domain_main::domain_main;
use ksync::Mutex;

pub type DomainInfoSet = Mutex<DomainInfo>;

pub fn domain_info() -> Arc<DomainInfoSet> {
    let res = corelib::domain_info().unwrap();
    unsafe { res.downcast_unchecked() }
}

pub fn catch_unwind<F: FnOnce() -> AlienResult<R>, R>(f: F) -> AlienResult<R> {
    let res = unwinding::panic::catch_unwind(f).unwrap_or_else(|_| {
        println_color!(31, "catch unwind error");
        Err(AlienError::DOMAINCRASH)
    });
    res
}

pub fn unwind_from_panic() {
    unwinding::panic::begin_panic(Box::new(()));
}

use getrandom::Error;

#[no_mangle]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), Error> {
    let buf = core::slice::from_raw_parts_mut(dest, len);

    let mut count = 0;
    while count < len {
        let now = format!("{}", time::read_timer());
        let bytes = now.as_bytes();
        let copy_len = core::cmp::min(len - count, bytes.len());
        buf[count..count + copy_len].copy_from_slice(&bytes[..copy_len]);
        count += copy_len;
    }
    Ok(())
}
