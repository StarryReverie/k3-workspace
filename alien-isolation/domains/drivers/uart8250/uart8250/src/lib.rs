#![no_std]
#![forbid(unsafe_code)]
mod vf2_uart;
extern crate alloc;
use alloc::boxed::Box;
use core::{
    fmt::{Debug, Formatter},
    ops::Range,
};

use basic::{
    io::SafeIORegion,
    println,
    sync::{Once, OnceGet},
    AlienResult,
};
use interface::{define_unwind_for_UartDomain, Basic, DeviceBase, UartDomain};
use shared_heap::DVec;

use crate::vf2_uart::Uart8250;
#[derive(Default)]
struct Uart8250Domain {
    uart: Once<Uart8250<4>>,
}

impl DeviceBase for Uart8250Domain {
    fn handle_irq(&self) -> AlienResult<()> {
        todo!()
    }
}

impl Debug for Uart8250Domain {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("Uart8250"))
    }
}

impl Basic for Uart8250Domain {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl UartDomain for Uart8250Domain {
    fn init(&self, address_range: &Range<usize>) -> AlienResult<()> {
        let region = address_range;
        println!("uart_addr: {:#x}-{:#x}", region.start, region.end);
        // let io_region = SafeIORegion::from(region.clone());
        let uart = Uart8250::<4>::new(SafeIORegion::from(region.clone()));
        self.uart.call_once(|| uart);
        self.enable_receive_interrupt().unwrap();
        println!("init uart success");
        Ok(())
    }

    fn putc(&self, ch: u8) -> AlienResult<()> {
        let uart = self.uart.get_must();
        uart.putc(ch)
    }
    fn getc(&self) -> AlienResult<Option<u8>> {
        self.uart.get_must().getc()
    }

    fn put_bytes(&self, buf: &DVec<u8>) -> AlienResult<usize> {
        let uart = self.uart.get_must();
        for i in 0..buf.len() {
            uart.putc(buf[i])?;
        }
        Ok(buf.len())
    }

    fn have_data_to_get(&self) -> AlienResult<bool> {
        self.uart.get_must().have_data_to_get()
    }

    fn enable_receive_interrupt(&self) -> AlienResult<()> {
        self.uart.get_must().enable_receive_interrupt()
    }

    fn disable_receive_interrupt(&self) -> AlienResult<()> {
        self.uart.get_must().disable_receive_interrupt()
    }
}

define_unwind_for_UartDomain!(Uart8250Domain);

pub fn main() -> Box<dyn UartDomain> {
    Box::new(UnwindWrap::new(Uart8250Domain::default()))
}
