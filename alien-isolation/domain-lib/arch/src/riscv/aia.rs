//! Safe wrappers around the RISC-V AIA IMSIC indirect CSRs.
//!
//! Domains run as supervisor-mode modules, but their crates forbid
//! `unsafe_code`. Keeping the CSR instructions here lets the interrupt domain
//! configure its own IMSIC file without opening an `unsafe` hole in the
//! domain.

use core::arch::asm;

/// IMSIC `eidelivery` register index.
pub const IMSIC_EIDELIVERY: usize = 0x70;
/// IMSIC `eithreshold` register index.
pub const IMSIC_EITHRESHOLD: usize = 0x72;
/// First IMSIC `eie` register index. Each word covers 64 interrupt IDs and
/// occupies two `siselect` indices.
pub const IMSIC_EIE: usize = 0xc0;

#[inline]
fn select(index: usize) {
    unsafe {
        asm!("csrw 0x150, {}", in(reg) index);
    }
}

#[inline]
fn read_selected() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, 0x151", out(reg) value);
    }
    value
}

#[inline]
fn write_selected(value: usize) {
    unsafe {
        asm!("csrw 0x151, {}", in(reg) value);
    }
}

/// Write one IMSIC register through `siselect`/`sireg`.
#[inline]
pub fn imsic_write(index: usize, value: usize) {
    select(index);
    write_selected(value);
}

/// Read one IMSIC register through `siselect`/`sireg`.
#[inline]
pub fn imsic_read(index: usize) -> usize {
    select(index);
    read_selected()
}

/// Claim the highest-priority pending interrupt from the current hart's
/// IMSIC file and clear its pending bit.
///
/// Returns 0 when there is no pending interrupt. Otherwise the EIID is in
/// bits 16..27, matching the AIA `stopei` layout.
#[inline]
pub fn imsic_claim() -> usize {
    let value: usize;
    unsafe {
        asm!("csrrw {}, 0x15c, zero", out(reg) value);
    }
    value
}
