use basic::{io::SafeIORegion, println, AlienResult};

pub struct Uart8250<const W: usize> {
    region: SafeIORegion,
}

impl<const W: usize> Uart8250<W> {
    pub fn new(region: SafeIORegion) -> Self {
        Self { region }
    }

    /// Configure the RX interrupt. Returns whether the UART is now
    /// interrupt-driven; `false` means the caller must keep polling.
    pub fn enable_receive_interrupt(&self, use_irq: bool) -> AlienResult<bool> {
        if basic::config::IS_K3 {
            // XScale UART: IER bit6 (UUE) enables the unit and MCR bit3
            // (OUT2) gates the interrupt line. Drain stale data first so a
            // level-high IRQ does not fire for bytes received before the
            // controller was ready.
            const IER_RDA: u32 = 0x01;
            const IER_UUE: u32 = 0x40;
            const MCR_OUT2: u32 = 0x08;
            const FCR_ENABLE_RX1: u32 = 0x01;
            while self.getc()?.is_some() {}
            self.region.write_at::<u32>(W * 2, FCR_ENABLE_RX1).unwrap();
            let mcr = self.region.read_at::<u32>(W * 4).unwrap();
            self.region.write_at::<u32>(W * 4, mcr | MCR_OUT2).unwrap();
            let ier = self.region.read_at::<u32>(W).unwrap();
            let mut value = ier | IER_UUE;
            value = if use_irq {
                value | IER_RDA
            } else {
                value & !IER_RDA
            };
            self.region.write_at::<u32>(W, value).unwrap();
            let lsr = self.region.read_at::<u32>(W * 5).unwrap();
            println!(
                "uart8250: IER={:#x} MCR={:#x} LSR={:#x} use_irq={}",
                value, mcr | MCR_OUT2, lsr, use_irq
            );
            return Ok(use_irq);
        }
        let ier = self.region.read_at::<u32>(W).unwrap() as u8;
        let value = if use_irq { ier | 1 } else { ier & !1 };
        self.region.write_at::<u32>(W, value as u32).unwrap();
        Ok(use_irq)
    }

    pub fn disable_receive_interrupt(&self) -> AlienResult<()> {
        let ier = self.region.read_at::<u32>(W).unwrap() as u8;
        self.region.write_at::<u32>(W, (ier & !1) as u32).unwrap();
        Ok(())
    }

    fn loop_putc(&self, ch: u8) -> AlienResult<()> {
        loop {
            let lsr = self.region.read_at::<u32>(W * 5).unwrap();
            if lsr & 0x20 != 0 {
                self.region.write_at::<u32>(0, ch as _).unwrap();
                break;
            }
        }
        Ok(())
    }

    pub fn putc(&self, ch: u8) -> AlienResult<()> {
        if ch == b'\n' {
            self.loop_putc(b'\r')?;
        }
        self.loop_putc(ch)
    }

    pub fn getc(&self) -> AlienResult<Option<u8>> {
        let lsr = self.region.read_at::<u32>(W * 5).unwrap() as u8;
        if lsr & 1 != 0 {
            return Ok(Some(self.region.read_at::<u32>(0).unwrap() as _));
        }
        Ok(None)
    }

    pub fn have_data_to_get(&self) -> AlienResult<bool> {
        let lsr = self.region.read_at::<u32>(W * 5).unwrap();
        Ok(lsr & 1 != 0)
    }
}
