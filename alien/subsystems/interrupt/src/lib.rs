#![no_std]
extern crate alloc;

use alloc::{collections::BTreeMap, format, string::String, sync::Arc};

use arch::hart_id;
use config::CPU_NUM;
use device_interface::DeviceBase;
use ksync::Mutex;
use platform::println;
use plic::{Mode, PLIC};
use spin::Once;

mod aia;

pub use aia::{init_aia, AiaConfig};

pub static PLIC: Once<PLIC<CPU_NUM>> = Once::new();
pub static INTERRUPT_RECORD: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());
pub static DEVICE_TABLE: Mutex<BTreeMap<usize, Arc<dyn DeviceBase>>> = Mutex::new(BTreeMap::new());

#[allow(dead_code)]
enum IrqController {
    Plic,
    Aia(aia::AiaController),
}

static CONTROLLER: Once<IrqController> = Once::new();

pub(crate) fn set_aia_controller(controller: aia::AiaController) {
    CONTROLLER.call_once(|| IrqController::Aia(controller));
}

pub fn init_plic(plic_addr: usize) {
    #[cfg(feature = "qemu")]
    {
        let privileges = [2; CPU_NUM];
        let plic = PLIC::new(plic_addr, privileges);
        PLIC.call_once(|| plic);
        CONTROLLER.call_once(|| IrqController::Plic);
        println!("Init qemu plic success");
    }
    #[cfg(feature = "k3")]
    {
        let _ = plic_addr;
        println!("K3: PLIC init skipped");
    }
    #[cfg(any(feature = "vf2", feature = "hifive"))]
    {
        let mut privileges = [2; CPU_NUM];
        // core 0 don't have S mode
        privileges[0] = 1;
        println!("PLIC context: {:?}", privileges);
        let plic = PLIC::new(plic_addr, privileges);
        PLIC.call_once(|| plic);
        CONTROLLER.call_once(|| IrqController::Plic);
        println!("Init hifive or vf2 plic success");
    }
}

impl IrqController {
    fn register(&self, irq: usize) {
        match self {
            Self::Plic => {
                let plic = PLIC.get().unwrap();
                let hart_id = hart_id();
                println!(
                    "PLIC enable irq {} for hart {}, priority {}",
                    irq, hart_id, 1
                );
                plic.set_threshold(hart_id as u32, Mode::Machine, 1);
                plic.set_threshold(hart_id as u32, Mode::Supervisor, 0);
                plic.complete(hart_id as u32, Mode::Supervisor, irq as u32);
                plic.set_priority(irq as u32, 1);
                plic.enable(hart_id as u32, Mode::Supervisor, irq as u32);
            }
            Self::Aia(controller) => controller.register(irq),
        }
    }

    fn claim(&self) -> Option<usize> {
        match self {
            Self::Plic => {
                let plic = PLIC.get().unwrap();
                let irq = plic.claim(hart_id() as u32, Mode::Supervisor);
                if irq == 0 {
                    None
                } else {
                    Some(irq as usize)
                }
            }
            Self::Aia(controller) => controller.claim(),
        }
    }

    fn complete(&self, irq: usize) {
        match self {
            Self::Plic => {
                let plic = PLIC.get().unwrap();
                plic.complete(hart_id() as u32, Mode::Supervisor, irq as u32);
            }
            Self::Aia(controller) => controller.complete(irq),
        }
    }
}

/// Register a device to the active interrupt controller.
pub fn register_device(irq: usize, device: Arc<dyn DeviceBase>) {
    DEVICE_TABLE.lock().insert(irq, device);
    match CONTROLLER.get() {
        Some(controller) => controller.register(irq),
        None => {
            println!("no interrupt controller for irq {}", irq);
        }
    }
}

/// Backward-compatible alias for PLIC-era callers.
pub fn register_device_to_plic(irq: usize, device: Arc<dyn DeviceBase>) {
    register_device(irq, device);
}

pub fn external_interrupt_handler() {
    let controller = match CONTROLLER.get() {
        Some(controller) => controller,
        None => {
            println!("external interrupt without controller");
            return;
        }
    };
    let Some(irq) = controller.claim() else {
        return;
    };
    {
        let table = DEVICE_TABLE.lock();
        match table.get(&irq) {
            Some(device) => device.handle_irq(),
            None => {
                println!("no device for irq {}", irq);
            }
        }
    }
    controller.complete(irq);
    record_irq(irq);
}

/// Serializes the number of interrupts
///
/// # Return
/// irq{}: number
pub fn interrupts_info() -> String {
    let interrupts = INTERRUPT_RECORD.lock();
    let mut res = String::new();
    interrupts.iter().for_each(|(irq, value)| {
        res.push_str(&format!("{}: {}\r\n", irq, value));
    });
    res
}

/// Increase the number of interrupts
pub fn record_irq(irq: usize) {
    let mut interrupts = INTERRUPT_RECORD.lock();
    let value = interrupts.entry(irq).or_insert(0);
    *value += 1;
}
