#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    sync::Arc,
};
use core::{
    cmp::min,
    fmt::{Debug, Formatter},
    sync::atomic::{AtomicBool, Ordering},
};

use basic::{
    config::MAX_CPU_NUM, io::SafeIORegion, println, sync::Mutex, AlienError, AlienResult,
};
use interface::{
    define_unwind_for_PLICDomain, AiaInfo, Basic, DeviceBase, PLICDomain, PlicInfo, PlicType,
};
use raw_plic::{Mode, PlicIO, PLIC};
use shared_heap::DVec;
use spin::Once;

static PLIC: Once<PLIC<MAX_CPU_NUM>> = Once::new();

/// APLIC register offsets. The `*IENUM` offsets are the AIA-defined
/// set/clear-by-number registers and do not depend on the source count.
const APLIC_DOMAINCFG: usize = 0x0000;
const APLIC_SOURCECFG: usize = 0x0004;
const APLIC_CLRIPNUM: usize = 0x1ddc;
const APLIC_SETIENUM: usize = 0x1edc;
const APLIC_CLRIENUM: usize = 0x1fdc;
const APLIC_TARGET: usize = 0x3004;

/// AIA table 2: level-high source mode.
const APLIC_SOURCECFG_LEVEL_HIGH: u32 = 6;

/// `domaincfg`: MSI delivery plus interrupt enable (bit31 is a read-only
/// feature flag on both QEMU and K3).
const APLIC_DOMAINCFG_MSI_ENABLE: u32 = 0x104;
const APLIC_DOMAINCFG_INIT: u32 = 0x100;

#[derive(Debug)]
pub struct SafeIORegionWrapper(SafeIORegion);

impl PlicIO for SafeIORegionWrapper {
    fn read_at(&self, offset: usize) -> u32 {
        self.0.read_at(offset).unwrap()
    }

    fn write_at(&self, offset: usize, value: u32) {
        self.0.write_at(offset, value).unwrap()
    }
}

enum DeviceDomain {
    Name(String),
    Domain(Arc<dyn DeviceBase>),
}

impl Debug for DeviceDomain {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            DeviceDomain::Name(name) => write!(f, "Name({})", name),
            DeviceDomain::Domain(_) => write!(f, "Domain"),
        }
    }
}

/// State of the AIA controller configured for this domain.
#[derive(Debug)]
struct AiaState {
    aplic: SafeIORegion,
    imsic_num_ids: usize,
    aplic_num_sources: usize,
    target_hart: usize,
}

#[derive(Debug)]
pub struct PLICDomainImpl {
    table: Mutex<BTreeMap<usize, DeviceDomain>>,
    count: Mutex<BTreeMap<usize, usize>>,
    enabled: AtomicBool,
    aia: Mutex<Option<AiaState>>,
}

impl Default for PLICDomainImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl PLICDomainImpl {
    pub fn new() -> Self {
        Self {
            table: Mutex::new(BTreeMap::new()),
            count: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
            aia: Mutex::new(None),
        }
    }

    /// Configure the S-level IMSIC file of the current hart and the APLIC.
    ///
    /// `stopei`/`siselect`/`sireg` are S-mode CSRs. The `arch` crate exposes
    /// safe wrappers so this domain can stay `forbid(unsafe_code)`.
    fn init_aia(&self, info: &AiaInfo) -> AlienResult<()> {
        let aplic = SafeIORegion::from(info.aplic.clone());
        let hart = arch::hart_id();
        aplic
            .write_at::<u32>(APLIC_DOMAINCFG, APLIC_DOMAINCFG_INIT)
            .map_err(|_| AlienError::EINVAL)?;

        // Disable delivery before touching EIE, then clear every enable bit.
        arch::aia::imsic_write(arch::aia::IMSIC_EIDELIVERY, 0);
        let words = info.imsic_num_ids / 64 + 1;
        for index in 0..words {
            arch::aia::imsic_write(arch::aia::IMSIC_EIE + index * 2, 0);
        }
        arch::aia::imsic_write(arch::aia::IMSIC_EITHRESHOLD, 0);
        arch::aia::imsic_write(arch::aia::IMSIC_EIDELIVERY, 1);

        let eidelivery = arch::aia::imsic_read(arch::aia::IMSIC_EIDELIVERY);
        let threshold = arch::aia::imsic_read(arch::aia::IMSIC_EITHRESHOLD);
        let domaincfg = aplic
            .read_at::<u32>(APLIC_DOMAINCFG)
            .map_err(|_| AlienError::EINVAL)?;
        println!(
            "AIA: eidelivery={:#x} eithreshold={:#x} domaincfg={:#x}",
            eidelivery, threshold, domaincfg
        );
        println!(
            "AIA: imsic={:#x?} ids={} aplic={:#x?} sources={}",
            info.imsic,
            info.imsic_num_ids,
            info.aplic,
            info.aplic_num_sources
        );
        println!(
            "AIA: hart_bits={} guest_bits={} target_hart={}",
            info.hart_index_bits, info.guest_index_bits, hart
        );

        *self.aia.lock() = Some(AiaState {
            aplic,
            imsic_num_ids: info.imsic_num_ids,
            aplic_num_sources: info.aplic_num_sources,
            target_hart: hart,
        });
        self.enabled.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Enable one APLIC source and its IMSIC EIID.
    fn enable_aia_source(&self, irq: usize) -> AlienResult<()> {
        let aia = self.aia.lock();
        let Some(state) = aia.as_ref() else {
            return Ok(());
        };
        if irq == 0 || irq >= state.aplic_num_sources || irq >= state.imsic_num_ids {
            println!(
                "AIA: irq {} out of range (sources={}, ids={})",
                irq, state.aplic_num_sources, state.imsic_num_ids
            );
            return Ok(());
        }
        let sourcecfg = APLIC_SOURCECFG + 4 * (irq - 1);
        let target_reg = APLIC_TARGET + 4 * (irq - 1);
        let target = ((state.target_hart << 18) | (irq & 0x7ff)) as u32;
        // Reset the source first. A UART line that is already high must not be
        // left in an unknown pending state from firmware setup.
        state
            .aplic
            .write_at::<u32>(sourcecfg, 0)
            .map_err(|_| AlienError::EINVAL)?;
        state
            .aplic
            .write_at::<u32>(APLIC_CLRIENUM, irq as u32)
            .map_err(|_| AlienError::EINVAL)?;
        state
            .aplic
            .write_at::<u32>(APLIC_CLRIPNUM, irq as u32)
            .map_err(|_| AlienError::EINVAL)?;
        state
            .aplic
            .write_at::<u32>(sourcecfg, APLIC_SOURCECFG_LEVEL_HIGH)
            .map_err(|_| AlienError::EINVAL)?;
        state
            .aplic
            .write_at::<u32>(target_reg, target)
            .map_err(|_| AlienError::EINVAL)?;
        state
            .aplic
            .write_at::<u32>(APLIC_SETIENUM, irq as u32)
            .map_err(|_| AlienError::EINVAL)?;

        let eie_index = arch::aia::IMSIC_EIE + 2 * (irq / 64);
        let old = arch::aia::imsic_read(eie_index);
        arch::aia::imsic_write(eie_index, old | (1usize << (irq % 64)));

        // Only enable APLIC delivery once the source and EIID are ready.
        state
            .aplic
            .write_at::<u32>(APLIC_DOMAINCFG, APLIC_DOMAINCFG_MSI_ENABLE)
            .map_err(|_| AlienError::EINVAL)?;
        println!(
            "AIA: enabled irq {} for hart {}, sourcecfg={:#x} target={:#x}",
            irq, state.target_hart, APLIC_SOURCECFG_LEVEL_HIGH, target
        );
        Ok(())
    }

    fn dispatch(&self, irq: usize) -> AlienResult<()> {
        let mut table = self.table.lock();
        let Some(device_domain) = table.get(&irq) else {
            // An interrupt can arrive while the remaining device domains are
            // still loading. Mask the EIID instead of panicking or storming.
            if self.aia.lock().is_some() {
                let eie_index = arch::aia::IMSIC_EIE + 2 * (irq / 64);
                let old = arch::aia::imsic_read(eie_index);
                arch::aia::imsic_write(eie_index, old & !(1usize << (irq % 64)));
            }
            println!("irq {} has no device, masked", irq);
            return Ok(());
        };
        match device_domain {
            DeviceDomain::Name(name) => {
                let device_domain = basic::get_domain(name).unwrap();
                let device_domain: Arc<dyn DeviceBase> = device_domain.try_into()?;
                device_domain.handle_irq()?;
                table.insert(irq, DeviceDomain::Domain(device_domain));
            }
            DeviceDomain::Domain(device) => {
                device.handle_irq()?;
            }
        }
        drop(table);

        let mut count = self.count.lock();
        if let Some(value) = count.get_mut(&irq) {
            *value += 1;
            if *value == 1 {
                println!("AIA/PLIC: irq {} delivered", irq);
            }
        }
        Ok(())
    }
}

impl Basic for PLICDomainImpl {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl PLICDomain for PLICDomainImpl {
    fn init(&self, plic_info: &PlicInfo) -> AlienResult<()> {
        println!("plic region: {:#x?}", plic_info.device_info);
        match plic_info.ty {
            PlicType::Aia => {
                let Some(aia) = plic_info.aia.as_ref() else {
                    println!("AIA: PlicType::Aia without parameters");
                    self.enabled.store(false, Ordering::Relaxed);
                    return Ok(());
                };
                return self.init_aia(aia);
            }
            PlicType::None => {
                self.enabled.store(false, Ordering::Relaxed);
                println!("plic disabled: external interrupts are not routed");
                return Ok(());
            }
            PlicType::Qemu | PlicType::SiFive => {}
        }
        let plic_space = SafeIORegion::from(plic_info.device_info.clone());
        let privileges = match plic_info.ty {
            PlicType::Qemu => [2; MAX_CPU_NUM],
            PlicType::SiFive => {
                let mut privileges = [2; MAX_CPU_NUM];
                // core 0 don't have S mode
                privileges[0] = 1;
                privileges
            }
            // Handled by the early return above.
            PlicType::Aia | PlicType::None => unreachable!(),
        };
        PLIC.call_once(|| PLIC::new(Box::new(SafeIORegionWrapper(plic_space)), privileges));
        println!("Init qemu plic success");
        Ok(())
    }

    fn handle_irq(&self) -> AlienResult<()> {
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        if self.aia.lock().is_some() {
            let value = arch::aia::imsic_claim();
            if value == 0 {
                return Ok(());
            }
            let irq = (value >> 16) & 0x7ff;
            return self.dispatch(irq);
        }
        let plic = PLIC.get().unwrap();
        let hart_id = arch::hart_id();
        let irq = plic.claim(hart_id as u32, Mode::Supervisor) as usize;
        self.dispatch(irq)?;
        plic.complete(hart_id as u32, Mode::Supervisor, irq as u32);
        Ok(())
    }

    fn register_irq(&self, irq: usize, device_domain_name: &DVec<u8>) -> AlienResult<()> {
        if !self.enabled.load(Ordering::Relaxed) || irq == 0 {
            return Ok(());
        }
        if self.aia.lock().is_some() {
            self.enable_aia_source(irq)?;
        } else {
            let hard_id = arch::hart_id();
            println!(
                "PLIC enable irq {} for hart {}, priority {}",
                irq, hard_id, 1
            );
            let plic = PLIC.get().unwrap();
            plic.set_threshold(hard_id as u32, Mode::Machine, 1);
            plic.set_threshold(hard_id as u32, Mode::Supervisor, 0);
            plic.complete(hard_id as u32, Mode::Supervisor, irq as u32);
            plic.set_priority(irq as u32, 1);
            plic.enable(hard_id as u32, Mode::Supervisor, irq as u32);
        }
        let mut table = self.table.lock();
        let device_domain_name = core::str::from_utf8(device_domain_name.as_slice()).unwrap();
        let domain = DeviceDomain::Name(device_domain_name.to_string());
        table.insert(irq, domain);
        self.count.lock().insert(irq, 0);
        Ok(())
    }

    fn irq_info(&self, mut buf: DVec<u8>) -> AlienResult<DVec<u8>> {
        let interrupts = self.count.lock();
        let mut res = String::new();
        interrupts.iter().for_each(|(irq, value)| {
            res.push_str(&format!("{}: {}\r\n", irq, value));
        });
        let copy_len = min(buf.len(), res.as_bytes().len());
        buf.as_mut_slice()[..copy_len].copy_from_slice(&res.as_bytes()[..copy_len]);
        Ok(buf)
    }
}

define_unwind_for_PLICDomain!(PLICDomainImpl);

pub fn main() -> Box<dyn PLICDomain> {
    let domain_impl = PLICDomainImpl::new();
    Box::new(UnwindWrap::new(domain_impl))
}
