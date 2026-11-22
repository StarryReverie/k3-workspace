use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::DVec;

use super::AlienResult;
use crate::{Basic, DeviceBase};

#[proxy(BufUartDomainProxy, RwLock, String)]
pub trait BufUartDomain: DeviceBase + Basic + DowncastSync {
    fn init(&self, uart_domain_name: &str) -> AlienResult<()>;
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
}

impl_downcast!(sync BufUartDomain);
