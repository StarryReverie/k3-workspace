use alloc::{boxed::Box, collections::VecDeque, sync::Arc};

use device_interface::{DeviceBase, UartDevice};
use ksync::Mutex;
use shim::KTask;

pub use self::{uart16550::Uart16550, uart8250::Uart8250};

pub trait LowUartDriver: Send + Sync {
    fn _init(&mut self);
    fn _put(&mut self, c: u8);
    fn _read(&mut self) -> Option<u8>;
    /// 返回 (IER, MCR) 的读回值，仅用于平台诊断。
    fn irq_state(&self) -> Option<(u32, u32)> {
        None
    }
}

mod uart8250 {
    use crate::uart::LowUartDriver;

    /// 接收数据可用中断（IER bit0）。
    const IER_RDA: u32 = 0x01;
    /// XScale 型 UART 的单元使能位（IER bit6）。
    const IER_UUE: u32 = 0x40;
    /// 中断输出使能（MCR bit3）。XScale 型 UART 需要该位为 1 才会拉高中断线。
    const MCR_OUT2: u32 = 0x08;
    /// 寄存器索引：0 为 THR/RBR，1 为 IER，2 为 IIR/FCR，4 为 MCR。
    const REG_IER: usize = 1;
    const REG_FCR: usize = 2;
    const REG_MCR: usize = 4;
    /// FIFO 使能，接收触发阈值 1 字节。
    const FCR_ENABLE_RX1: u32 = 0x01;

    pub struct Uart8250 {
        uart_raw: uart8250::MmioUart8250<'static, u32>,
        base_addr: usize,
        xscale: bool,
    }

    unsafe impl Send for Uart8250 {}

    unsafe impl Sync for Uart8250 {}

    impl Uart8250 {
        pub fn new(base_addr: usize) -> Self {
            let uart_raw = unsafe { uart8250::MmioUart8250::<u32>::new(base_addr) };
            Uart8250 {
                uart_raw,
                base_addr,
                xscale: false,
            }
        }

        /// 构造 XScale 型 8250，用于 K3 与 K1。
        ///
        /// 该变体把 IER 的 bit6 作为 UART 单元使能位。通用的
        /// “读出 IER、只置接收中断位”写法会清掉该位，令 UART 停止工作。
        pub fn new_xscale(base_addr: usize) -> Self {
            let uart_raw = unsafe { uart8250::MmioUart8250::<u32>::new(base_addr) };
            Uart8250 {
                uart_raw,
                base_addr,
                xscale: true,
            }
        }

        /// 读取 32 位寄存器。K3 使用 reg-shift=2，寄存器步长为 4 字节。
        fn read_reg(&self, index: usize) -> u32 {
            unsafe { ((self.base_addr + index * 4) as *const u32).read_volatile() }
        }

        fn write_reg(&self, index: usize, value: u32) {
            unsafe { ((self.base_addr + index * 4) as *mut u32).write_volatile(value) }
        }
    }

    impl LowUartDriver for Uart8250 {
        fn _init(&mut self) {
            if !self.xscale {
                self.uart_raw.enable_received_data_available_interrupt();
                return;
            }
            // 丢弃引导阶段残留的接收数据，避免使能后立即触发中断。
            while self.uart_raw.read_byte().is_some() {}
            self.write_reg(REG_FCR, FCR_ENABLE_RX1);
            // XScale 型 UART 的中断线由 MCR 的 OUT2 位门控。
            let mcr = self.read_reg(REG_MCR) | MCR_OUT2;
            self.write_reg(REG_MCR, mcr);
            let ier = self.read_reg(REG_IER) | IER_UUE | IER_RDA;
            self.write_reg(REG_IER, ier);
        }

        fn _put(&mut self, c: u8) {
            loop {
                if self.uart_raw.write_byte(c).is_ok() {
                    break;
                }
            }
        }

        fn _read(&mut self) -> Option<u8> {
            self.uart_raw.read_byte()
        }

        fn irq_state(&self) -> Option<(u32, u32)> {
            Some((self.read_reg(REG_IER) & 0xff, self.read_reg(REG_MCR) & 0xff))
        }
    }
}

mod uart16550 {
    use crate::uart::LowUartDriver;

    pub struct Uart16550 {
        uart_raw: &'static mut uart16550::Uart16550<u8>,
    }

    unsafe impl Send for Uart16550 {}

    unsafe impl Sync for Uart16550 {}

    impl Uart16550 {
        pub fn new(base_addr: usize) -> Self {
            let uart_raw = unsafe { &mut *(base_addr as *mut uart16550::Uart16550<u8>) };
            Uart16550 { uart_raw }
        }
    }

    impl LowUartDriver for Uart16550 {
        fn _init(&mut self) {
            use uart16550::InterruptTypes;
            let ier = self.uart_raw.ier();
            let inter = InterruptTypes::ZERO;
            ier.write(inter.enable_rda());
        }

        fn _put(&mut self, c: u8) {
            self.uart_raw.write(&[c]);
        }

        fn _read(&mut self) -> Option<u8> {
            let mut buf = [0];
            let r = self.uart_raw.read(&mut buf);
            if r == 0 {
                None
            } else {
                Some(buf[0])
            }
        }
    }
}

pub struct Uart {
    inner: Mutex<(Box<dyn LowUartDriver>, UartInner)>,
}

struct UartInner {
    rx_buf: VecDeque<u8>,
    wait_queue: VecDeque<Arc<dyn KTask>>,
}

impl Uart {
    pub fn new(uart_raw: Box<dyn LowUartDriver>) -> Self {
        Self::new_with_init(uart_raw, true)
    }

    /// 创建 UART，但暂不使能中断。
    ///
    /// 某些平台需要先配置中断控制器，再使能 UART 的 IER。
    pub fn new_deferred(uart_raw: Box<dyn LowUartDriver>) -> Self {
        Self::new_with_init(uart_raw, false)
    }

    fn new_with_init(uart_raw: Box<dyn LowUartDriver>, init: bool) -> Self {
        let mut uart_raw = uart_raw;
        if init {
            uart_raw._init();
        }
        let inner = UartInner {
            rx_buf: VecDeque::new(),
            wait_queue: VecDeque::new(),
        };
        Uart {
            inner: Mutex::new((uart_raw, inner)),
        }
    }

    /// 在中断控制器配置完成后使能 UART 接收中断。
    ///
    /// 返回底层驱动的 (IER, MCR) 读回值，用于确认使能位已被硬件接受。
    pub fn enable_irq(&self) -> Option<(u32, u32)> {
        let mut inner = self.inner.lock();
        inner.0._init();
        inner.0.irq_state()
    }
}

impl UartDevice for Uart {
    fn put(&self, c: u8) {
        let mut inner = self.inner.lock();
        inner.0._put(c);
    }
    fn get(&self) -> Option<u8> {
        loop {
            let mut inner = self.inner.lock();
            if inner.1.rx_buf.is_empty() {
                let task = shim::take_current_task().unwrap();
                task.to_wait();
                inner.1.wait_queue.push_back(task.clone());
                drop(inner);
                shim::schedule_now(task);
            } else {
                return inner.1.rx_buf.pop_front();
            }
        }
    }

    fn put_bytes(&self, bytes: &[u8]) {
        for &c in bytes {
            if c == b'\n' {
                self.put(b'\r');
            }
            self.put(c);
        }
    }

    fn have_data_to_get(&self) -> bool {
        !self.inner.lock().1.rx_buf.is_empty()
    }

    fn have_space_to_put(&self) -> bool {
        true
    }
}

impl DeviceBase for Uart {
    fn handle_irq(&self) {
        loop {
            let mut inner = self.inner.lock();
            if let Some(c) = inner.0._read() {
                inner.1.rx_buf.push_back(c);
                if !inner.1.wait_queue.is_empty() {
                    let task = inner.1.wait_queue.pop_front().unwrap();
                    task.to_wakeup();
                    shim::put_task(task);
                }
            } else {
                break;
            }
        }
    }
}
