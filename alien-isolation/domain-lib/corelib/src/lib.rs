#![no_std]
extern crate alloc;

use alloc::sync::Arc;
use core::any::Any;

#[cfg(feature = "core_impl")]
pub use core_impl::*;
use interface::{DomainType, DomainTypeRaw};
use pconst::LinuxErrno;
use spin::Once;
use task_meta::{OperationResult, TaskOperation};

pub mod domain_info;

pub type AlienError = LinuxErrno;
pub type AlienResult<T> = Result<T, LinuxErrno>;

pub mod constants {
    pub use pconst::*;

    use crate::AlienError;

    pub const AT_FDCWD: isize = -100isize;
    #[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq, Hash, Ord)]
    pub struct DeviceId {
        major: u32,
        minor: u32,
    }

    impl DeviceId {
        pub fn new(major: u32, minor: u32) -> Self {
            Self { major, minor }
        }
        pub fn id(&self) -> u64 {
            ((self.major as u64) << 32) | (self.minor as u64)
        }
    }

    impl From<u64> for DeviceId {
        fn from(id: u64) -> Self {
            Self {
                major: (id >> 32) as u32,
                minor: (id & 0xffffffff) as u32,
            }
        }
    }
    #[derive(Debug)]
    pub enum PriorityTarget {
        Process(u32),
        ProcessGroup(u32),
        User(u32),
    }
    #[allow(non_camel_case_types)]
    #[derive(Clone, Debug)]
    #[repr(i32)]
    pub enum Which {
        PRIO_PROCESS = 0,
        PRIO_PGRP = 1,
        PRIO_USER = 2,
    }

    impl TryFrom<i32> for Which {
        type Error = AlienError;

        fn try_from(value: i32) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(Which::PRIO_PROCESS),
                1 => Ok(Which::PRIO_PGRP),
                2 => Ok(Which::PRIO_USER),
                _ => Err(AlienError::EINVAL),
            }
        }
    }
}

pub trait CoreFunction: Send + Sync {
    fn sys_alloc_pages(&self, domain_id: u64, n: usize) -> *mut u8;
    fn sys_free_pages(&self, domain_id: u64, p: *mut u8, n: usize);
    fn sys_write_console(&self, s: &str);
    fn sys_backtrace(&self, domain_id: u64);
    fn sys_trampoline_addr(&self) -> usize;
    fn sys_kernel_satp(&self) -> usize;
    fn sys_trap_from_user(&self) -> usize;
    fn sys_trap_to_user(&self) -> usize;
    /// This func will be deleted
    fn blk_crash_trick(&self) -> bool;
    fn sys_get_domain(&self, name: &str) -> Option<DomainType>;
    fn sys_create_domain(
        &self,
        domain_file_name: &str,
        identifier: &mut [u8],
    ) -> AlienResult<DomainType>;
    /// Register a new domain with the given name and type
    fn sys_register_domain(&self, ident: &str, ty: DomainTypeRaw, data: &[u8]) -> AlienResult<()>;
    /// Replace the old domain with the new domain
    fn sys_update_domain(
        &self,
        old_domain_name: &str,
        new_domain_name: &str,
        ty: DomainTypeRaw,
    ) -> AlienResult<()>;
    fn sys_reload_domain(&self, domain_name: &str) -> AlienResult<()>;
    fn vaddr_to_paddr_in_kernel(&self, vaddr: usize) -> AlienResult<usize>;
    fn task_op(&self, op: TaskOperation) -> AlienResult<OperationResult>;
    fn checkout_shared_data(&self) -> AlienResult<()>;

    fn domain_info(&self) -> AlienResult<Arc<dyn Any + Send + Sync>>;
}

#[cfg(feature = "core_impl")]
mod core_impl {
    use alloc::sync::Arc;
    use core::any::Any;

    use interface::{DomainType, DomainTypeRaw};
    use spin::Once;
    use task_meta::{TaskMeta, TaskOperation};

    use super::{AlienError, AlienResult, OnceGet};
    use crate::CoreFunction;

    static CORE_FUNC: Once<&'static dyn CoreFunction> = Once::new();

    extern "C" {
        fn sbss();
        fn ebss();
    }
    fn clear_bss() {
        unsafe {
            core::slice::from_raw_parts_mut(
                sbss as usize as *mut u8,
                ebss as usize - sbss as usize,
            )
            .fill(0);
        }
    }

    pub fn init(syscall: &'static dyn CoreFunction) {
        clear_bss();
        CORE_FUNC.call_once(|| syscall);
    }

    pub fn alloc_raw_pages(n: usize, domain_id: u64) -> *mut u8 {
        CORE_FUNC.get_must().sys_alloc_pages(domain_id, n)
    }

    pub fn free_raw_pages(p: *mut u8, n: usize, domain_id: u64) {
        CORE_FUNC.get_must().sys_free_pages(domain_id, p, n);
    }

    pub fn write_console(s: &str) {
        CORE_FUNC.get_must().sys_write_console(s);
    }

    pub fn backtrace(domain_id: u64) {
        CORE_FUNC.get_must().sys_backtrace(domain_id);
    }

    pub fn trampoline_addr() -> usize {
        static TRAMPOLINE_ADDR: Once<usize> = Once::new();

        TRAMPOLINE_ADDR.call_once(|| CORE_FUNC.get_must().sys_trampoline_addr());
        *TRAMPOLINE_ADDR.get_must()
    }

    pub fn kernel_satp() -> usize {
        static SATP: Once<usize> = Once::new();

        SATP.call_once(|| CORE_FUNC.get_must().sys_kernel_satp());
        *SATP.get_must()
    }

    pub fn trap_from_user() -> usize {
        static TRAP_FROM_USER: Once<usize> = Once::new();

        TRAP_FROM_USER.call_once(|| CORE_FUNC.get_must().sys_trap_from_user());
        *TRAP_FROM_USER.get_must()
    }

    pub fn trap_to_user() -> usize {
        static TRAP_TO_USER: Once<usize> = Once::new();

        TRAP_TO_USER.call_once(|| CORE_FUNC.get_must().sys_trap_to_user());
        *TRAP_TO_USER.get_must()
    }

    // todo!(delete)
    pub fn blk_crash_trick() -> bool {
        CORE_FUNC.get_must().blk_crash_trick()
    }

    pub fn get_domain(name: &str) -> Option<DomainType> {
        CORE_FUNC.get_must().sys_get_domain(name)
    }

    pub fn create_domain(
        domain_file_name: &str,
        domain_identifier: &mut [u8],
    ) -> AlienResult<DomainType> {
        if domain_identifier.len() < 32 {
            return Err(AlienError::EINVAL);
        }
        CORE_FUNC
            .get_must()
            .sys_create_domain(domain_file_name, domain_identifier)
    }

    pub fn register_domain(ident: &str, ty: DomainTypeRaw, data: &[u8]) -> AlienResult<()> {
        CORE_FUNC.get_must().sys_register_domain(ident, ty, data)
    }

    pub fn update_domain(
        old_domain_name: &str,
        new_domain_name: &str,
        ty: DomainTypeRaw,
    ) -> AlienResult<()> {
        CORE_FUNC
            .get_must()
            .sys_update_domain(old_domain_name, new_domain_name, ty)
    }

    pub fn reload_domain(domain_name: &str) -> AlienResult<()> {
        CORE_FUNC.get_must().sys_reload_domain(domain_name)
    }
    pub fn vaddr_to_paddr_in_kernel(vaddr: usize) -> AlienResult<usize> {
        CORE_FUNC.get_must().vaddr_to_paddr_in_kernel(vaddr)
    }

    #[inline(always)]
    fn current_tid_from_tp() -> Option<usize> {
        let mut tp: usize;
        unsafe {
            core::arch::asm!(
                "mv {}, tp",
                out(reg) tp,
            )
        }
        tp = tp >> 32;
        if tp == 0 {
            None
        } else {
            Some(tp)
        }
    }
    #[inline(always)]
    pub fn current_tid() -> AlienResult<Option<usize>> {
        // CORE_FUNC
        //     .get_must()
        //     .task_op(TaskOperation::Current)
        //     .map(|res| res.current_tid())
        Ok(current_tid_from_tp())
    }
    /// return kstack top
    pub fn add_one_task(task_meta: TaskMeta) -> AlienResult<usize> {
        CORE_FUNC
            .get_must()
            .task_op(TaskOperation::Create(task_meta))
            .map(|res| res.kstack_top())
    }
    /// Set current task to wait and switch to next task
    pub fn wait_now() -> AlienResult<()> {
        CORE_FUNC.get_must().task_op(TaskOperation::Wait)?;
        Ok(())
    }
    /// Wake up the task with tid
    pub fn wake_up_wait_task(tid: usize) -> AlienResult<()> {
        CORE_FUNC.get_must().task_op(TaskOperation::Wakeup(tid))?;
        Ok(())
    }
    /// Yield the current task
    pub fn yield_now() -> AlienResult<()> {
        CORE_FUNC.get_must().task_op(TaskOperation::Yield)?;
        Ok(())
    }
    pub fn exit_now() -> AlienResult<()> {
        CORE_FUNC.get_must().task_op(TaskOperation::Exit)?;
        Ok(())
    }
    /// remove task from scheduler, release resources
    pub fn remove_task(tid: usize) -> AlienResult<()> {
        CORE_FUNC.get_must().task_op(TaskOperation::Remove(tid))?;
        Ok(())
    }

    /// Check if the task is exit over
    pub fn is_task_exit(tid: usize) -> AlienResult<bool> {
        CORE_FUNC
            .get_must()
            .task_op(TaskOperation::ExitOver(tid))
            .map(|res| res.is_exit_over())
    }

    pub fn set_task_priority(priority: i8) -> AlienResult<()> {
        CORE_FUNC
            .get_must()
            .task_op(TaskOperation::SetPriority(priority))?;
        Ok(())
    }

    pub fn get_task_priority() -> AlienResult<i8> {
        CORE_FUNC
            .get_must()
            .task_op(TaskOperation::GetPriority)
            .map(|res| res.priority())
    }

    pub fn checkout_shared_data() -> AlienResult<()> {
        CORE_FUNC.get_must().checkout_shared_data()
    }

    pub fn domain_info() -> AlienResult<Arc<dyn Any + Send + Sync>> {
        CORE_FUNC.get_must().domain_info()
    }
}

impl<T> OnceGet<T> for Once<T> {
    fn get_must(&self) -> &T {
        unsafe { self.get_unchecked() }
    }
}

pub trait OnceGet<T> {
    fn get_must(&self) -> &T;
}
