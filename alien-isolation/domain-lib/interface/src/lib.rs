#![no_std]
#![feature(trait_upcasting)]
mod block;
mod buf_input;
mod buf_uart;
mod cache_block;
mod empty_device;
mod fs;
mod gpu;
mod input_device;
mod logger;
mod net;
mod net_device;
mod plic;
mod rtc;
mod scheduler;
mod shadow_block;
mod syscall;
mod task;
mod uart;
mod vfs;

extern crate alloc;

use alloc::sync::Arc;
use core::{
    any::Any,
    fmt::{Debug, Display},
};

use pconst::LinuxErrno;

type AlienError = LinuxErrno;
type AlienResult<T> = Result<T, LinuxErrno>;

pub trait Basic: Send + Sync + Debug + Any {
    fn domain_id(&self) -> u64;

    #[cfg(feature = "domain")]
    fn is_active(&self) -> bool {
        __impl::is_active()
    }
    #[cfg(not(feature = "domain"))]
    fn is_active(&self) -> bool {
        false
    }
}

pub trait DeviceBase: Send + Sync {
    fn handle_irq(&self) -> AlienResult<()>;
}

pub use block::*;
pub use buf_input::*;
pub use buf_uart::*;
pub use cache_block::*;
pub use empty_device::*;
pub use fs::*;
pub use gpu::*;
pub use input_device::*;
pub use logger::*;
pub use net::*;
pub use net_device::*;
pub use plic::*;
pub use rtc::*;
pub use scheduler::*;
pub use shadow_block::*;
pub use syscall::*;
pub use task::*;
pub use uart::*;
pub use vfs::*;

#[derive(Clone, Debug)]
pub enum DomainType {
    FsDomain(Arc<dyn FsDomain>),
    BlkDeviceDomain(Arc<dyn BlkDeviceDomain>),
    CacheBlkDeviceDomain(Arc<dyn CacheBlkDeviceDomain>),
    RtcDomain(Arc<dyn RtcDomain>),
    GpuDomain(Arc<dyn GpuDomain>),
    InputDomain(Arc<dyn InputDomain>),
    VfsDomain(Arc<dyn VfsDomain>),
    UartDomain(Arc<dyn UartDomain>),
    PLICDomain(Arc<dyn PLICDomain>),
    TaskDomain(Arc<dyn TaskDomain>),
    SysCallDomain(Arc<dyn SysCallDomain>),
    ShadowBlockDomain(Arc<dyn ShadowBlockDomain>),
    BufUartDomain(Arc<dyn BufUartDomain>),
    NetDeviceDomain(Arc<dyn NetDeviceDomain>),
    BufInputDomain(Arc<dyn BufInputDomain>),
    EmptyDeviceDomain(Arc<dyn EmptyDeviceDomain>),
    DevFsDomain(Arc<dyn DevFsDomain>),
    SchedulerDomain(Arc<dyn SchedulerDomain>),
    LogDomain(Arc<dyn LogDomain>),
    NetDomain(Arc<dyn NetDomain>),
}

impl DomainType {
    pub fn to_raw(&self) -> DomainTypeRaw {
        match self {
            DomainType::FsDomain(_) => DomainTypeRaw::FsDomain,
            DomainType::BlkDeviceDomain(_) => DomainTypeRaw::BlkDeviceDomain,
            DomainType::CacheBlkDeviceDomain(_) => DomainTypeRaw::CacheBlkDeviceDomain,
            DomainType::RtcDomain(_) => DomainTypeRaw::RtcDomain,
            DomainType::GpuDomain(_) => DomainTypeRaw::GpuDomain,
            DomainType::InputDomain(_) => DomainTypeRaw::InputDomain,
            DomainType::VfsDomain(_) => DomainTypeRaw::VfsDomain,
            DomainType::UartDomain(_) => DomainTypeRaw::UartDomain,
            DomainType::PLICDomain(_) => DomainTypeRaw::PLICDomain,
            DomainType::TaskDomain(_) => DomainTypeRaw::TaskDomain,
            DomainType::SysCallDomain(_) => DomainTypeRaw::SysCallDomain,
            DomainType::ShadowBlockDomain(_) => DomainTypeRaw::ShadowBlockDomain,
            DomainType::BufUartDomain(_) => DomainTypeRaw::BufUartDomain,
            DomainType::NetDeviceDomain(_) => DomainTypeRaw::NetDeviceDomain,
            DomainType::BufInputDomain(_) => DomainTypeRaw::BufInputDomain,
            DomainType::EmptyDeviceDomain(_) => DomainTypeRaw::EmptyDeviceDomain,
            DomainType::DevFsDomain(_) => DomainTypeRaw::DevFsDomain,
            DomainType::SchedulerDomain(_) => DomainTypeRaw::SchedulerDomain,
            DomainType::LogDomain(_) => DomainTypeRaw::LogDomain,
            DomainType::NetDomain(_) => DomainTypeRaw::NetDomain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum DomainTypeRaw {
    FsDomain = 1,
    BlkDeviceDomain = 2,
    CacheBlkDeviceDomain = 3,
    RtcDomain = 4,
    GpuDomain = 5,
    InputDomain = 6,
    VfsDomain = 7,
    UartDomain = 8,
    PLICDomain = 9,
    TaskDomain = 10,
    SysCallDomain = 11,
    ShadowBlockDomain = 12,
    BufUartDomain = 13,
    NetDeviceDomain = 14,
    BufInputDomain = 15,
    EmptyDeviceDomain = 16,
    DevFsDomain = 17,
    SchedulerDomain = 18,
    LogDomain = 19,
    NetDomain = 20,
}

impl Display for DomainTypeRaw {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DomainTypeRaw::FsDomain => write!(f, "FsDomain"),
            DomainTypeRaw::BlkDeviceDomain => write!(f, "BlkDeviceDomain"),
            DomainTypeRaw::CacheBlkDeviceDomain => write!(f, "CacheBlkDeviceDomain"),
            DomainTypeRaw::RtcDomain => write!(f, "RtcDomain"),
            DomainTypeRaw::GpuDomain => write!(f, "GpuDomain"),
            DomainTypeRaw::InputDomain => write!(f, "InputDomain"),
            DomainTypeRaw::VfsDomain => write!(f, "VfsDomain"),
            DomainTypeRaw::UartDomain => write!(f, "UartDomain"),
            DomainTypeRaw::PLICDomain => write!(f, "PLICDomain"),
            DomainTypeRaw::TaskDomain => write!(f, "TaskDomain"),
            DomainTypeRaw::SysCallDomain => write!(f, "SysCallDomain"),
            DomainTypeRaw::ShadowBlockDomain => write!(f, "ShadowBlockDomain"),
            DomainTypeRaw::BufUartDomain => write!(f, "BufUartDomain"),
            DomainTypeRaw::NetDeviceDomain => write!(f, "NetDeviceDomain"),
            DomainTypeRaw::BufInputDomain => write!(f, "BufInputDomain"),
            DomainTypeRaw::EmptyDeviceDomain => write!(f, "EmptyDeviceDomain"),
            DomainTypeRaw::DevFsDomain => write!(f, "DevFsDomain"),
            DomainTypeRaw::SchedulerDomain => write!(f, "SchedulerDomain"),
            DomainTypeRaw::LogDomain => write!(f, "LogDomain"),
            DomainTypeRaw::NetDomain => write!(f, "NetDomain"),
        }
    }
}

impl TryFrom<u8> for DomainTypeRaw {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DomainTypeRaw::FsDomain),
            2 => Ok(DomainTypeRaw::BlkDeviceDomain),
            3 => Ok(DomainTypeRaw::CacheBlkDeviceDomain),
            4 => Ok(DomainTypeRaw::RtcDomain),
            5 => Ok(DomainTypeRaw::GpuDomain),
            6 => Ok(DomainTypeRaw::InputDomain),
            7 => Ok(DomainTypeRaw::VfsDomain),
            8 => Ok(DomainTypeRaw::UartDomain),
            9 => Ok(DomainTypeRaw::PLICDomain),
            10 => Ok(DomainTypeRaw::TaskDomain),
            11 => Ok(DomainTypeRaw::SysCallDomain),
            12 => Ok(DomainTypeRaw::ShadowBlockDomain),
            13 => Ok(DomainTypeRaw::BufUartDomain),
            14 => Ok(DomainTypeRaw::NetDeviceDomain),
            15 => Ok(DomainTypeRaw::BufInputDomain),
            16 => Ok(DomainTypeRaw::EmptyDeviceDomain),
            17 => Ok(DomainTypeRaw::DevFsDomain),
            18 => Ok(DomainTypeRaw::SchedulerDomain),
            19 => Ok(DomainTypeRaw::LogDomain),
            20 => Ok(DomainTypeRaw::NetDomain),
            _ => Err(()),
        }
    }
}

impl TryInto<Arc<dyn DeviceBase>> for DomainType {
    type Error = AlienError;

    fn try_into(self) -> Result<Arc<dyn DeviceBase>, Self::Error> {
        match self {
            DomainType::BlkDeviceDomain(domain) => Ok(domain),
            DomainType::CacheBlkDeviceDomain(domain) => Ok(domain),
            DomainType::RtcDomain(domain) => Ok(domain),
            DomainType::GpuDomain(domain) => Ok(domain),
            DomainType::InputDomain(domain) => Ok(domain),
            DomainType::UartDomain(domain) => Ok(domain),
            DomainType::ShadowBlockDomain(domain) => Ok(domain),
            DomainType::BufUartDomain(domain) => Ok(domain),
            DomainType::BufInputDomain(domain) => Ok(domain),
            DomainType::NetDeviceDomain(domain) => Ok(domain),
            DomainType::NetDomain(domain) => Ok(domain),
            _ => Err(AlienError::EINVAL),
        }
    }
}

impl DomainType {
    pub fn domain_id(&self) -> u64 {
        match self {
            DomainType::FsDomain(d) => d.domain_id(),
            DomainType::BlkDeviceDomain(d) => d.domain_id(),
            DomainType::CacheBlkDeviceDomain(d) => d.domain_id(),
            DomainType::RtcDomain(d) => d.domain_id(),
            DomainType::GpuDomain(d) => d.domain_id(),
            DomainType::InputDomain(d) => d.domain_id(),
            DomainType::VfsDomain(d) => d.domain_id(),
            DomainType::UartDomain(d) => d.domain_id(),
            DomainType::PLICDomain(d) => d.domain_id(),
            DomainType::TaskDomain(d) => d.domain_id(),
            DomainType::SysCallDomain(d) => d.domain_id(),
            DomainType::ShadowBlockDomain(d) => d.domain_id(),
            DomainType::BufUartDomain(d) => d.domain_id(),
            DomainType::NetDeviceDomain(d) => d.domain_id(),
            DomainType::BufInputDomain(d) => d.domain_id(),
            DomainType::EmptyDeviceDomain(d) => d.domain_id(),
            DomainType::DevFsDomain(d) => d.domain_id(),
            DomainType::SchedulerDomain(d) => d.domain_id(),
            DomainType::LogDomain(d) => d.domain_id(),
            DomainType::NetDomain(d) => d.domain_id(),
        }
    }
}

#[cfg(feature = "domain")]
mod __impl {
    use core::{hint::spin_loop, sync::atomic::AtomicBool};

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// Activate the domain
    ///
    /// It should be called in the `main` function of the domain.
    pub fn activate_domain() {
        ACTIVE.store(true, core::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn is_active() -> bool {
        ACTIVE.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// Deactivate the domain
    ///
    /// It should be called in the `panic` function of the domain it should block the thread which
    /// calls this function when the `ACTIVE` flag is false.
    pub fn deactivate_domain() {
        while !ACTIVE.swap(false, core::sync::atomic::Ordering::Relaxed) {
            spin_loop();
        }
    }
}

#[cfg(feature = "domain")]
pub use __impl::*;
