#![feature(naked_functions)]
#![feature(box_into_inner)]
#![feature(trait_upcasting)]
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(ptr_metadata)]
#![feature(let_chains)]
#![allow(clippy::declare_interior_mutable_const)]
#![no_std]
#![no_main]
#![feature(lang_items)]
#![allow(internal_features)]
mod panic;

#[macro_use]
extern crate platform;
#[macro_use]
extern crate log;
extern crate alloc;
mod bus;
mod domain;
mod domain_helper;
mod domain_loader;
mod domain_proxy;
mod error;
mod sync;
mod task;
mod timer;
mod trap;

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, Ordering},
};

/// 多核启动标志
static STARTED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
fn main(hart_id: usize) {
    #[cfg(feature = "k3-diag")]
    diag_main(hart_id);
    #[cfg(not(feature = "k3-diag"))]
    normal_main(hart_id);
}

/// K3 diagnostic path: verify DTB, memory, paging and secondary harts.
///
/// The domain set still assumes PLIC, virtio and the QEMU UART, so it is not
/// started here. M3 replaces the interrupt and console paths before domains
/// are enabled on K3.
#[cfg(feature = "k3-diag")]
fn diag_main(hart_id: usize) {
    if platform::boot_hart() == hart_id {
        STARTED.store(true, Ordering::Relaxed);
        println!("Boot hart {}", hart_id);
        let machine_info = platform::platform_machine_info();
        println!("{:#?}", machine_info);
        mem::init_memory_system(machine_info.memory.end, true);
        arch::allow_access_user_memory();
        println!("K3_BOOT_OK");
        STARTED.store(false, Ordering::Relaxed);
    } else {
        while STARTED.load(Ordering::Relaxed) {
            spin_loop();
        }
        mem::init_memory_system(0, false);
        arch::allow_access_user_memory();
        println!("hart {} start...", arch::hart_id());
    }
    loop {
        spin_loop();
    }
}

#[cfg(not(feature = "k3-diag"))]
fn normal_main(hart_id: usize) {
    if platform::boot_hart() == hart_id {
        STARTED.store(true, Ordering::Relaxed);
        println!("Boot hart {}", hart_id);
        let machine_info = platform::platform_machine_info();
        println!("{:#?}", machine_info);
        mem::init_memory_system(machine_info.memory.end, true);
        arch::allow_access_user_memory();
        bus::init_with_dtb().unwrap();
        trap::init_trap_subsystem();

        domain::load_domains().unwrap();
        STARTED.store(false, Ordering::Relaxed);
    } else {
        while STARTED.load(Ordering::Relaxed) {
            spin_loop();
        }
        mem::init_memory_system(0, false);
        arch::allow_access_user_memory();
        trap::init_trap_subsystem();
        println!("hart {} start...", arch::hart_id());
    }
    #[cfg(feature = "test")]
    panic::test_unwind();
    timer::set_next_trigger();
    println!("Begin run task...");
    task::run_task();
}
