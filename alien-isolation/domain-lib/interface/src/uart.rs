use core::ops::Range;

use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::{AlienError, AlienResult};
use crate::{Basic, DeviceBase};

/// Parameters passed to a UART domain.
#[derive(Clone, Debug)]
pub struct UartInfo {
    pub device_info: Range<usize>,
    /// Whether the platform routes the UART IRQ through an interrupt
    /// controller. When false, the console is polled.
    pub use_irq: bool,
}

#[proxy(UartDomainProxy,RwLock,UartInfo)]
pub trait UartDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, device_info: &UartInfo) -> AlienResult<()>;
    /// Write a character to the UART
    fn putc(&self, ch: u8) -> AlienResult<()>;
    /// Read a character from the UART
    fn getc(&self) -> AlienResult<Option<u8>>;

    fn put_bytes(&self, buf: &DVec<u8>) -> AlienResult<usize>;
    /// Check if there is data to get from the UART
    fn have_data_to_get(&self) -> AlienResult<bool>;
    /// Check if there is space to put data to the UART
    fn have_space_to_put(&self) -> AlienResult<bool> {
        Ok(true)
    }
    /// Enable receive interrupts. Returns whether the UART is now
    /// interrupt-driven; `false` means the caller must poll.
    fn enable_receive_interrupt(&self) -> AlienResult<bool>;
    fn disable_receive_interrupt(&self) -> AlienResult<()>;
    fn enable_transmit_interrupt(&self) -> AlienResult<()> {
        Err(AlienError::ENOSYS)
    }
    fn disable_transmit_interrupt(&self) -> AlienResult<()> {
        Err(AlienError::ENOSYS)
    }
}

impl_downcast!(sync UartDomain);
