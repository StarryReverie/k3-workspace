use arch::hart_id;
use platform::println;

/// AIA 控制器初始化参数。
#[derive(Debug, Clone, Copy)]
pub struct AiaConfig {
    pub imsic_base: usize,
    pub imsic_size: usize,
    pub imsic_num_ids: usize,
    pub hart_index_bits: u8,
    pub guest_index_bits: u8,
    pub aplic_base: usize,
    pub aplic_size: usize,
    pub aplic_num_sources: usize,
    pub uart_irq: usize,
    pub target_hart: usize,
}

pub struct AiaController {
    config: AiaConfig,
}

const APLIC_DOMAINCFG: usize = 0x0000;
const APLIC_SOURCECFG: usize = 0x0004;
const APLIC_CLRIPNUM: usize = 0x1ddc;
const APLIC_SETIENUM: usize = 0x1edc;
const APLIC_CLRIENUM: usize = 0x1fdc;
const APLIC_TARGET: usize = 0x3004;

const IMSIC_EIDELIVERY: usize = 0x70;
const IMSIC_EITHRESHOLD: usize = 0x72;
const IMSIC_EIE: usize = 0xc0;

unsafe fn siselect(value: usize) {
    core::arch::asm!("csrw 0x150, {}", in(reg) value);
}

unsafe fn sireg_read() -> usize {
    let value: usize;
    core::arch::asm!("csrr {}, 0x151", out(reg) value);
    value
}

unsafe fn sireg_write(value: usize) {
    core::arch::asm!("csrw 0x151, {}", in(reg) value);
}

unsafe fn stopei_claim() -> usize {
    let value: usize;
    core::arch::asm!("csrrw {}, 0x15c, zero", out(reg) value);
    value
}

unsafe fn write32(address: usize, value: u32) {
    (address as *mut u32).write_volatile(value);
}

impl AiaController {
    pub fn new(config: AiaConfig) -> Self {
        Self { config }
    }

    /// 初始化 APLIC 与当前 hart 的 IMSIC S-file。
    pub fn init(&self) -> bool {
        if self.config.imsic_base == 0 || self.config.aplic_base == 0 {
            println!("AIA: missing IMSIC or APLIC");
            return false;
        }
        if hart_id() != self.config.target_hart {
            println!(
                "AIA: init on hart {} but target is {}",
                hart_id(),
                self.config.target_hart
            );
            return false;
        }

        let aplic = self.config.aplic_base;
        unsafe {
            write32(aplic + APLIC_DOMAINCFG, 0x100);
            if self.config.uart_irq > 0 {
                write32(aplic + APLIC_SOURCECFG + 4 * (self.config.uart_irq - 1), 0);
                write32(aplic + APLIC_CLRIENUM, self.config.uart_irq as u32);
                write32(aplic + APLIC_CLRIPNUM, self.config.uart_irq as u32);
            }

            // 先关闭投递并清空 eie，避免复位状态未定义的使能位立即触发中断。
            siselect(IMSIC_EIDELIVERY);
            sireg_write(0);
            let words = self.config.imsic_num_ids / 64 + 1;
            for index in 0..words {
                siselect(IMSIC_EIE + index * 2);
                sireg_write(0);
            }
            siselect(IMSIC_EITHRESHOLD);
            sireg_write(0);
            siselect(IMSIC_EIDELIVERY);
            sireg_write(1);
        }

        println!(
            "AIA: imsic={:#x} size={:#x} ids={} aplic={:#x} size={:#x} sources={}",
            self.config.imsic_base,
            self.config.imsic_size,
            self.config.imsic_num_ids,
            self.config.aplic_base,
            self.config.aplic_size,
            self.config.aplic_num_sources
        );
        println!(
            "AIA: hart_bits={} guest_bits={}",
            self.config.hart_index_bits, self.config.guest_index_bits
        );
        true
    }

    pub fn register(&self, irq: usize) {
        if irq == 0 || irq >= self.config.aplic_num_sources {
            println!("AIA: invalid irq {}", irq);
            return;
        }
        if hart_id() != self.config.target_hart {
            println!(
                "AIA: register irq {} on hart {} but target is {}",
                irq,
                hart_id(),
                self.config.target_hart
            );
            return;
        }
        if irq >= self.config.imsic_num_ids {
            println!("AIA: irq {} exceeds IMSIC ids {}", irq, self.config.imsic_num_ids);
            return;
        }

        let aplic = self.config.aplic_base;
        let target = ((self.config.target_hart << 18) | (irq & 0x7ff)) as u32;
        unsafe {
            write32(aplic + APLIC_SOURCECFG + 4 * (irq - 1), 6);
            write32(aplic + APLIC_TARGET + 4 * (irq - 1), target);
            write32(aplic + APLIC_SETIENUM, irq as u32);

            siselect(IMSIC_EIE + 2 * (irq / 64));
            let old = sireg_read();
            sireg_write(old | (1usize << (irq % 64)));

            // 设备源配置完成后才打开 APLIC 全局投递。
            write32(aplic + APLIC_DOMAINCFG, 0x104);
        }
        println!("AIA: enabled irq {} for hart {}", irq, self.config.target_hart);
    }

    pub fn claim(&self) -> Option<usize> {
        let value = unsafe { stopei_claim() };
        if value == 0 {
            None
        } else {
            Some((value >> 16) & 0x7ff)
        }
    }

    pub fn complete(&self, _irq: usize) {
        // stopei 的读写已经清除 pending 位，无需额外 complete。
    }
}

/// 初始化 AIA 并注册为当前中断控制器。
pub fn init_aia(config: AiaConfig) -> bool {
    let controller = AiaController::new(config);
    if !controller.init() {
        return false;
    }
    crate::set_aia_controller(controller);
    true
}
