pub mod config;

use core::ops::Range;

use spin::Once;

use crate::PlatformInfo;

/// 第一版不使用设备树；保留入口参数，便于后续接入。
pub static DTB: Once<usize> = Once::new();

pub fn init_dtb(dtb: Option<usize>) {
    DTB.call_once(|| dtb.unwrap_or(0));
}

pub fn basic_machine_info() -> PlatformInfo {
    let mut model = [0u8; 32];
    #[cfg(feature = "k3_qemu")]
    let name = b"QEMU virt (K3 test)";
    #[cfg(not(feature = "k3_qemu"))]
    let name = b"SpacemiT K3";
    model[..name.len()].copy_from_slice(name);

    PlatformInfo {
        model,
        smp: ::config::CPU_NUM,
        #[cfg(feature = "k3_qemu")]
        memory: 0x8000_0000..0xc000_0000,
        #[cfg(not(feature = "k3_qemu"))]
        memory: 0x1_0200_0000..0x3_0000_0000,
        plic: 0..0,
        clint: 0..0,
        initrd: Some(Range {
            start: INITRD.as_ptr() as usize,
            end: INITRD.as_ptr() as usize + INITRD.len(),
        }),
        bootargs: None,
        bootargs_len: 0,
    }
}

static INITRD: &[u8] = include_bytes!("../../../../tools/initrd/initramfs.cpio.gz");

pub fn set_timer(time: usize) {
    crate::common_riscv::sbi::set_timer(time);
}

pub fn system_shutdown() -> ! {
    crate::common_riscv::sbi::system_shutdown();
}

pub fn console_putchar(ch: u8) {
    crate::common_riscv::sbi::console_putchar(ch);
}

#[allow(unused)]
pub fn console_getchar() -> char {
    crate::common_riscv::sbi::console_getchar()
}
