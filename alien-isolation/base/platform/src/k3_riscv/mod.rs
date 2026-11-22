pub mod config;

#[cfg(feature = "k3-initrd")]
use core::ops::Range;

use spin::Once;

use crate::PlatformInfo;

#[cfg(feature = "k3-initrd")]
static INITRD: &[u8] = include_bytes!("../../../../build/k3-initramfs.cpio.gz");

pub static DTB: Once<usize> = Once::new();

pub fn init_dtb(dtb: Option<usize>) {
    let dtb_ptr = dtb.expect("No dtb found");
    DTB.call_once(|| dtb_ptr);
}

pub fn basic_machine_info() -> PlatformInfo {
    let mut info = crate::common_riscv::basic::machine_info_from_dtb(*DTB.get().unwrap());
    #[cfg(feature = "k3-initrd")]
    {
        info.initrd = Some(Range {
            start: INITRD.as_ptr() as usize,
            end: INITRD.as_ptr() as usize + INITRD.len(),
        });
    }
    info
}

pub fn set_timer(time: usize) {
    crate::common_riscv::sbi::set_timer(time);
}

pub fn system_shutdown() -> ! {
    println!("shutdown...");
    crate::common_riscv::sbi::system_shutdown();
}

pub fn console_putchar(ch: u8) {
    crate::common_riscv::sbi::console_putchar(ch);
}
