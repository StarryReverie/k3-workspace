#![allow(unused)]
//! SBI 调用接口
use core::arch::asm;

/// 设置定时器
const SBI_SET_TIMER: usize = 0;
/// 控制台输出
const SBI_CONSOLE_PUT_CHAR: usize = 1;
/// 控制台输入
const SBI_CONSOLE_GET_CHAR: usize = 2;
// const SBI_CLEAR_IPI: usize = 3;
/// 发送 IPI
const SBI_SEND_IPI: usize = 4;
// const SBI_REMOTE_FENCE_I: usize = 5;
// const SBI_REMOTE_SFENCE_VMA: usize = 6;
// const SBI_REMOTE_SFENCE_VMA_ASID: usize = 7;
/// 关闭机器
const SBI_SHUTDOWN: usize = 8;

/// SBI 调用
///
/// sbi规范定义了调用的参数传递方法
fn sbi_call(which: usize, arg0: usize, arg1: usize, arg2: usize) -> i32 {
    let mut ret;
    unsafe {
        asm!("ecall",
        in("a7") which,
        inlateout("a0") arg0 as i32 => ret,
        in("a1") arg1,
        in("a2") arg2);
    }
    ret
}

/// 设置定时器
pub fn set_timer(time: usize) {
    let ret = sbi_call_3(EXTENSION_TIMER, FUNCTION_TIMER_SET_TIMER, time, 0, 0);
    if ret.error != 0 {
        sbi_call(SBI_SET_TIMER, time, 0, 0);
    }
}

pub fn system_shutdown() -> ! {
    let ret = sbi_call_3(
        EXTENSION_SRST,
        FUNCTION_SRST_SYSTEM_RESET,
        0, // shutdown
        0, // no reason
        0,
    );
    if ret.error != 0 {
        sbi_call(SBI_SHUTDOWN, 0, 0, 0);
    }
    loop {}
}

/// Warp sbi SBI_CONSOLE_PUT_CHAR  call
pub fn console_putchar(ch: u8) {
    let buf = [ch];
    let ret = sbi_call_3(
        EXTENSION_DBCN,
        FUNCTION_DBCN_WRITE,
        1,
        buf.as_ptr() as usize,
        0,
    );
    if ret.error != 0 || ret.value <= 0 {
        sbi_call(SBI_CONSOLE_PUT_CHAR, ch as usize, 0, 0);
    }
}

/// Warp sbi SBI_CONSOLE_GET_CHAR  call
#[allow(unused)]
pub fn console_getchar() -> char {
    let mut buf = [0u8; 1];
    let ret = sbi_call_3(
        EXTENSION_DBCN,
        FUNCTION_DBCN_READ,
        1,
        buf.as_mut_ptr() as usize,
        0,
    );
    if ret.error == 0 && ret.value == 1 {
        buf[0] as char
    } else {
        sbi_call(SBI_CONSOLE_GET_CHAR, 0, 0, 0) as u8 as char
    }
}

/// sbi调用返回值
#[repr(C)]
#[derive(Debug)]
pub struct SbiRet {
    /// Error number
    pub error: isize,
    /// Result value
    pub value: isize,
}

/// SBI 基本扩展
pub const EXTENSION_BASE: usize = 0x10;
/// SBI 时钟扩展
pub const EXTENSION_TIMER: usize = 0x54494D45;
// pub const EXTENSION_IPI: usize = 0x735049;
// pub const EXTENSION_RFENCE: usize = 0x52464E43;
/// SBI HSM 扩展
pub const EXTENSION_HSM: usize = 0x48534D;
/// SBI 系统复位扩展
pub const EXTENSION_SRST: usize = 0x53525354;
/// SBI 调试控制台扩展
pub const EXTENSION_DBCN: usize = 0x4442434E;

/// SBI TIME 扩展的设置定时器功能
const FUNCTION_TIMER_SET_TIMER: usize = 0;
/// SBI SRST 扩展的系统复位功能
const FUNCTION_SRST_SYSTEM_RESET: usize = 0;
/// SBI DBCN 扩展的写入功能
const FUNCTION_DBCN_WRITE: usize = 0;
/// SBI DBCN 扩展的读取功能
const FUNCTION_DBCN_READ: usize = 2;

/// SBI HSM扩展的启动cpu功能
const FUNCTION_HSM_HART_START: usize = 0x0;
// const FUNCTION_HSM_HART_STOP: usize = 0x1;
// const FUNCTION_HSM_HART_GET_STATUS: usize = 0x2;
const FUNCTION_HSM_HART_SUSPEND: usize = 0x3;

/// 第三种类型的SBI调用
///
/// 可以传递更多参数
#[inline(always)]
fn sbi_call_3(extension: usize, function: usize, arg0: usize, arg1: usize, arg2: usize) -> SbiRet {
    let (error, value);
    unsafe {
        asm!(
        "ecall",
        in("a0") arg0, in("a1") arg1, in("a2") arg2,
        in("a6") function, in("a7") extension,
        lateout("a0") error, lateout("a1") value,
        )
    }
    SbiRet { error, value }
}

pub fn hart_suspend(suspend_type: u32, resume_addr: usize, opaque: usize) -> SbiRet {
    sbi_call_3(
        EXTENSION_HSM,
        FUNCTION_HSM_HART_SUSPEND,
        suspend_type as usize,
        resume_addr,
        opaque,
    )
}

/// wrap sbi FUNCTION_HSM_HART_START call
pub fn hart_start(hart_id: usize, start_addr: usize, opaque: usize) -> SbiRet {
    sbi_call_3(
        EXTENSION_HSM,
        FUNCTION_HSM_HART_START,
        hart_id,
        start_addr,
        opaque,
    )
}

/// wrap sbi SBI_SEND_IPI call
#[allow(unused)]
pub fn send_ipi(ptr: usize) {
    sbi_call(SBI_SEND_IPI, ptr, 0, 0);
}
