pub mod config;

#[cfg(feature = "k3-initrd")]
use core::ops::Range;

use spin::Once;

use crate::PlatformInfo;

#[cfg(feature = "k3-initrd")]
static INITRD: &[u8] = include_bytes!("../../../../build/k3-initramfs.cpio.gz");

pub static DTB: Once<usize> = Once::new();

/// True when the control DTB describes a SpacemiT K3 board.
///
/// QEMU runs the K3 image with a `riscv-virtio,qemu` root node, so this is
/// also the gate for mapping K3-only PCIe MMIO.
pub fn is_k3_board() -> bool {
    let Some(ptr) = DTB.get() else {
        return false;
    };
    let Ok(fdt) = (unsafe { fdt::Fdt::from_ptr(*ptr as *const u8) }) else {
        return false;
    };
    fdt.root()
        .compatible()
        .all()
        .any(|value| value.contains("spacemit,k3"))
}

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
