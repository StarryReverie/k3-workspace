use basic::{config::CLOCK_FREQ, io::SafeIORegion, time::read_timer};
use visionfive2_sd::{SDIo, SleepOps};

pub struct SdIoImpl {
    io: SafeIORegion,
}

impl SdIoImpl {
    pub fn new(io: SafeIORegion) -> Self {
        SdIoImpl { io }
    }
}

// pub const SDIO_BASE: usize = 0x16020000;

impl SDIo for SdIoImpl {
    #[inline]
    fn read_reg_at(&self, offset: usize) -> u32 {
        self.io.read_at(offset).unwrap()
    }
    #[inline]
    fn write_reg_at(&mut self, offset: usize, val: u32) {
        self.io.write_at(offset, val).unwrap()
    }
    #[inline]
    fn read_data_at(&self, offset: usize) -> u64 {
        self.io.read_at(offset).unwrap()
    }
    #[inline]
    fn write_data_at(&mut self, offset: usize, val: u64) {
        self.io.write_at(offset, val).unwrap()
    }
}

pub struct SleepOpsImpl;

impl SleepOps for SleepOpsImpl {
    #[inline]
    fn sleep_ms(ms: usize) {
        sleep_ms(ms)
    }
    #[inline]
    fn sleep_ms_until(ms: usize, f: impl FnMut() -> bool) {
        sleep_ms_until(ms, f)
    }
}

#[inline]
fn sleep_ms(ms: usize) {
    let start = read_timer();
    while read_timer() - start < ms * CLOCK_FREQ / 1000 {
        core::hint::spin_loop();
    }
}

#[inline]
fn sleep_ms_until(ms: usize, mut f: impl FnMut() -> bool) {
    let start = read_timer();
    while read_timer() - start < ms * CLOCK_FREQ / 1000 {
        if f() {
            return;
        }
        core::hint::spin_loop();
    }
}
