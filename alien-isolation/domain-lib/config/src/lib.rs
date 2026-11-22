//! 配置文件
#![no_std]

#[cfg(qemu_riscv)]
mod qemu_riscv;
#[cfg(vf2)]
mod vf2;

#[cfg(qemu_riscv)]
pub use qemu_riscv::*;
#[cfg(vf2)]
pub use vf2::*;

/// Alien os的标志
pub const ALIEN_FLAG: &str = r"
     _      _   _
    / \    | | (_)   ___   _ __
   / _ \   | | | |  / _ \ | '_ \
  / ___ \  | | | | |  __/ | | | |
 /_/   \_\ |_| |_|  \___| |_| |_|
";

/// 物理页大小
pub const FRAME_SIZE: usize = 0x1000;
pub const PAGE_SIZE: usize = FRAME_SIZE;
/// 物理页大小的位数
pub const FRAME_BITS: usize = 12;
/// 内核启动栈大小
pub const STACK_SIZE: usize = 1024 * 64;
/// 内核启动栈大小的位数
pub const STACK_SIZE_BITS: usize = 16;

/// 可配置的启动cpu数量
pub const CPU_NUM: usize = 2;

const HEAP_SIZE: usize = 0x400_0000;
pub const KERNEL_HEAP_SIZE: usize = HEAP_SIZE;

pub const TRAMPOLINE: usize = usize::MAX - 2 * FRAME_SIZE + 1;

pub const TRAP_CONTEXT_BASE: usize = TRAMPOLINE - FRAME_SIZE;
pub const USER_KERNEL_STACK_SIZE: usize = FRAME_SIZE * 16;
pub const KTHREAD_STACK_SIZE: usize = FRAME_SIZE * 2;
/// 线程数量大小限制
pub const MAX_THREAD_NUM: usize = 65536;
/// 描述符数量大小限制
pub const MAX_FD_NUM: usize = 4096;

/// app用户栈大小
// pub const USER_STACK_SIZE: usize = 0x50_000;

pub const USER_STACK_SIZE: usize = 0x50_000;
pub const ELF_BASE_RELOCATE: usize = 0x400_0000;

pub const MAX_INPUT_EVENT_NUM: u32 = 1024;
pub const PROCESS_HEAP_MAX: usize = u32::MAX as usize + 1;

pub const PIPE_BUF: usize = 65536;
