#![feature(atomic_from_mut)]
// To handle compiler bugs
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![feature(breakpoint)]
#![no_std]

extern crate alloc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate platform;
#[macro_use]
extern crate syscall_table;
extern crate unwinder;
use alloc::boxed::Box;

pub use syscall_table::*;
mod ebpf;
mod fs;
mod gui;
mod ipc;
mod kprobe;
mod ksym;
mod mm;
mod net;
mod per_cpu;
mod perf;
mod system;
mod task;
mod time;
mod trap;

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use platform::platform_machine_info;

use crate::task::DriverTaskImpl;

/// 主 hart 完成初始化后置位。
static INIT_DONE: AtomicBool = AtomicBool::new(false);
/// 已完成初始化并进入调度的 hart 位图。
#[cfg(feature = "smp")]
static HART_ONLINE: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
fn main(hart_id: usize) {
    if hart_id == platform::boot_hart() {
        println!("Boot hart {}", hart_id);
        let machine_info = platform_machine_info();
        println!("{:#?}", machine_info);
        mem::init_memory_system(machine_info.memory.end, true);
        #[cfg(not(any(feature = "k3", feature = "k3_qemu")))]
        interrupt::init_plic(machine_info.plic.start);
        shim::register_task_func(Box::new(DriverTaskImpl));
        #[cfg(not(any(feature = "k3", feature = "k3_qemu")))]
        devices::init_device();
        #[cfg(feature = "k3")]
        devices::init_k3_uart(0xd401_7000);
        #[cfg(feature = "k3_qemu")]
        devices::init_k3_qemu_uart(0x1000_0000);
        vfs::init_filesystem().expect("init filesystem failed");
        ksym::init_kallsyms();
        trap::init_trap_subsystem();
        arch::allow_access_user_memory();
        task::init_task();
        // register all syscall
        syscall_table::init_init_array!();
        INIT_DONE.store(true, Ordering::Release);
        #[cfg(feature = "smp")]
        HART_ONLINE.fetch_or(1usize << hart_id, Ordering::Release);
    } else {
        while !INIT_DONE.load(Ordering::Acquire) {
            spin_loop();
        }
        mem::init_memory_system(0, false);
        arch::allow_access_user_memory();
        trap::init_trap_subsystem();
        #[cfg(feature = "smp")]
        HART_ONLINE.fetch_or(1usize << hart_id, Ordering::Release);
        println!("hart {} start", arch::hart_id());
    }
    #[cfg(feature = "smp")]
    if hart_id == platform::boot_hart() {
        let start = arch::read_timer();
        let timeout = platform::config::CLOCK_FREQ / 20;
        while HART_ONLINE.load(Ordering::Acquire) != config::CPU_MASK
            && arch::read_timer() - start < timeout
        {
            spin_loop();
        }
        println!(
            "SMP: {}/{} harts online",
            HART_ONLINE.load(Ordering::Acquire).count_ones(),
            config::CPU_NUM
        );
    }
    #[cfg(feature = "kprobe_test")]
    kprobe::kprobe_test::kprobe_test();
    time::set_next_trigger();
    println!("Begin run task...");
    task::schedule::run_task();
}
