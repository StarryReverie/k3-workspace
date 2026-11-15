#![no_std]

mod block;
mod gpu;
mod input;
mod net;
mod prob;
mod rtc;
mod uart;

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

pub use block::{BLKDevice, BLOCK_DEVICE};
use config::MAX_INPUT_EVENT_NUM;
use device_interface::{DeviceBase, GpuDevice, LowBlockDevice};
use drivers::{
    block_device::GenericBlockDevice,
    rtc::GoldFishRtc,
    uart::{LowUartDriver, Uart, Uart16550, Uart8250},
};
use fdt::Fdt;
pub use gpu::{GPUDevice, GPU_DEVICE};
pub use input::{INPUTDevice, KEYBOARD_INPUT_DEVICE, MOUSE_INPUT_DEVICE};
use interrupt::{init_aia, register_device, AiaConfig};
use log::info;
use platform::println;
pub use rtc::{RTCDevice, RTC_DEVICE};
pub use uart::{PollingUart, UARTDevice, UART_DEVICE};

static K3_IRQ_UART: spin::Once<Arc<Uart>> = spin::Once::new();
static K3_UART_IRQ: AtomicUsize = AtomicUsize::new(0);

/// 为 K3 注册控制台。
///
/// AIA 初始化成功且取得 UART 中断号时使用中断驱动输入；否则回退轮询。
/// K3 的 UART 属于 XScale 型，写 IER 时必须保留单元使能位。
pub fn init_k3_console(base_addr: usize, qemu_uart: bool) {
    let dtb_ptr = platform::platform_dtb_ptr();
    let dtb = unsafe { Fdt::from_ptr(dtb_ptr as *const u8) };
    let mut irq = 0;
    let mut aia_ok = false;
    if let Ok(dtb) = dtb {
        if let (Some(uart), Some(imsic), Some(aplic)) = (
            dtb.probe_uart(),
            dtb.probe_imsic(),
            dtb.probe_aplic(),
        ) {
            irq = if uart.irq != 0 {
                uart.irq
            } else {
                platform::config::UART_IRQ
            };
            aia_ok = init_aia(AiaConfig {
                imsic_base: imsic.base,
                imsic_size: imsic.size,
                imsic_num_ids: imsic.num_ids,
                hart_index_bits: imsic.hart_index_bits,
                guest_index_bits: imsic.guest_index_bits,
                aplic_base: aplic.base,
                aplic_size: aplic.size,
                aplic_num_sources: aplic.num_sources,
                uart_irq: irq,
                target_hart: platform::boot_hart(),
            });
        }
    }

    let raw: Box<dyn LowUartDriver> = if qemu_uart {
        Box::new(Uart16550::new(base_addr))
    } else {
        // K3 的 UART 为 XScale 型，IER bit6 是单元使能位。
        Box::new(Uart8250::new_xscale(base_addr))
    };

    if aia_ok && irq != 0 {
        let uart = Arc::new(Uart::new_deferred(raw));
        uart::init_uart(uart.clone());
        K3_IRQ_UART.call_once(|| uart);
        K3_UART_IRQ.store(irq, Ordering::Release);
        println!("K3 console: interrupt UART deferred, irq {}", irq);
    } else {
        let uart = Arc::new(PollingUart::new(raw));
        uart::init_uart(uart);
        println!("K3 console: polling");
    }
}

/// 在陷阱子系统就绪后注册 UART 中断并使能 IER。
pub fn enable_k3_uart_irq() {
    let Some(uart) = K3_IRQ_UART.get() else {
        return;
    };
    let irq = K3_UART_IRQ.load(Ordering::Acquire);
    if irq == 0 {
        return;
    }
    register_device(irq, uart.clone());
    let state = uart.enable_irq();
    if let Some((ier, mcr)) = state {
        println!(
            "K3 console: interrupt-driven, irq {} (IER={:#x} MCR={:#x})",
            irq, ier, mcr
        );
    } else {
        println!("K3 console: interrupt-driven, irq {}", irq);
    }
}

/// 为 K3 注册轮询模式的 ns16550。K3 使用 32 位寄存器与 reg-shift=2。
pub fn init_k3_uart(base_addr: usize) {
    init_k3_console(base_addr, false);
}

/// 为 K3 QEMU 回归注册 16550。QEMU virt 使用 8 位寄存器。
pub fn init_k3_qemu_uart(base_addr: usize) {
    init_k3_console(base_addr, true);
}

/// 打印 K3 设备树中的内存、CPU、UART、IMSIC 与 APLIC。
///
/// 阶段 7A 只读探测，用于核对硬编码平台信息。
pub fn probe_k3_platform() {
    let dtb_ptr = platform::platform_dtb_ptr();
    let dtb = unsafe { Fdt::from_ptr(dtb_ptr as *const u8) };
    let dtb = match dtb {
        Ok(dtb) => dtb,
        Err(error) => {
            println!("DT: parse failed: {:?}", error);
            return;
        }
    };

    if let Some(memory) = dtb.probe_memory() {
        println!("DT memory: {:#x}..{:#x}", memory.start, memory.end);
    }
    println!("DT cpus: {:?}", dtb.probe_harts());
    if let Some(uart) = dtb.probe_uart() {
        println!(
            "DT uart: base={:#x} irq={} compatible={}",
            uart.base_addr, uart.irq, uart.compatible
        );
    }
    if let Some(imsic) = dtb.probe_imsic() {
        println!(
            "DT imsic: base={:#x} size={:#x} num_ids={} hart_bits={} guest_bits={}",
            imsic.base, imsic.size, imsic.num_ids, imsic.hart_index_bits, imsic.guest_index_bits
        );
    }
    if let Some(aplic) = dtb.probe_aplic() {
        println!(
            "DT aplic: base={:#x} size={:#x} num_sources={}",
            aplic.base, aplic.size, aplic.num_sources
        );
    }
}
use virtio_drivers::transport::{
    mmio::{MmioTransport, VirtIOHeader},
    DeviceType, Transport,
};

use crate::prob::Probe;

pub struct DeviceInfo {
    pub device: Arc<dyn DeviceBase>,
    pub irq: usize,
    pub need_register: bool,
}

/// Probe all devices from device tree and init them.
/// # Warning
/// Before init device, we should init platform first.
///
pub fn init_device() {
    let dtb_ptr = platform::platform_dtb_ptr();

    let dtb = unsafe { Fdt::from_ptr(dtb_ptr as *const u8).unwrap() };
    match dtb.probe_rtc() {
        Some(rtc) => init_rtc(rtc),
        None => {
            println!("There is no rtc device");
        }
    }

    match dtb.probe_uart() {
        Some(uart) => init_uart(uart),
        None => {
            println!("There is no uart device");
        }
    }

    #[cfg(not(all(feature = "vf2", feature = "hifive")))]
    {
        match dtb.probe_virtio() {
            Some(virtio_mmio_devices) => init_virtio_mmio(virtio_mmio_devices),
            None => {
                println!("There is no virtio-mmio device");
            }
        }
    }

    #[cfg(feature = "vf2")]
    match dtb.probe_sdio() {
        Some(sdio) => init_block_device(sdio, None),
        None => {
            panic!("There is no sdio device");
        }
    }

    #[cfg(feature = "hifive")]
    init_block_device(
        prob::DeviceInfo::new(
            alloc::string::String::new(),
            0,
            0,
            alloc::string::String::new(),
        ),
        None,
    );

    #[cfg(any(feature = "hifive", feature = "vf2"))]
    init_net(None);
}

fn init_rtc(rtc: prob::DeviceInfo) {
    let info = rtc;
    println!(
        "Init rtc, base_addr:{:#x}, irq:{}",
        info.base_addr, info.irq
    );
    match info.compatible.as_str() {
        "google,goldfish-rtc" => {
            let rtc = Arc::new(GoldFishRtc::new(info.base_addr));
            let current_time = rtc.read_time_string();
            rtc::init_rtc(rtc.clone());
            register_device(info.irq, rtc);
            println!("Init rtc success, current time: {:?}", current_time);
        }
        name => {
            println!("Don't support rtc: {}", name);
        }
    }
}

fn init_uart(uart: prob::DeviceInfo) {
    let (base_addr, irq) = (uart.base_addr, uart.irq);
    println!("Init uart, base_addr:{:#x},irq:{}", base_addr, irq);
    match uart.compatible.as_str() {
        "ns16550a" => {
            // qemu
            let uart = Uart16550::new(base_addr);
            let uart = Arc::new(Uart::new(Box::new(uart)));
            uart::init_uart(uart.clone());
            register_device(irq, uart);
        }
        "snps,dw-apb-uart" => {
            // vf2
            let uart = Uart8250::new(base_addr);
            let uart = Arc::new(Uart::new(Box::new(uart)));
            uart::init_uart(uart.clone());
            register_device(irq, uart);
        }
        name => {
            panic!("Don't support uart: {}", name);
        }
    }
    println!("Init uart success");
}

// keyboard
const VIRTIO5: usize = 0x10005000;
// mouse
const VIRTIO6: usize = 0x10006000;

pub fn init_virtio_mmio(devices: Vec<prob::DeviceInfo>) {
    for device in devices {
        let paddr = device.base_addr;
        let header = NonNull::new(paddr as *mut VirtIOHeader).unwrap();
        match unsafe { MmioTransport::new(header) } {
            Err(_) => {}
            Ok(mut transport) => {
                info!(
                    "Detected virtio MMIO device with vendor id {:#X}, device type {:?}, version {:?}, features:{:?}",
                    transport.vendor_id(),
                    transport.device_type(),
                    transport.version(),
                    transport.read_device_features(),
                );
                info!("Probe virtio device: {:?}", transport.device_type());
                match transport.device_type() {
                    DeviceType::Input => {
                        if paddr == VIRTIO5 {
                            init_input_device(device, "keyboard", Some(transport));
                        } else if paddr == VIRTIO6 {
                            init_input_device(device, "mouse", Some(transport));
                        }
                    }
                    DeviceType::Block => init_block_device(device, Some(transport)),
                    DeviceType::GPU => init_gpu(device, Some(transport)),
                    DeviceType::Network => init_net(Some(device)),
                    ty => {
                        println!("Don't support virtio device type: {:?}", ty);
                    }
                }
            }
        }
    }
}

#[cfg(feature = "ramdisk")]
static RAMDISK: &'static [u8] = include_bytes!("../../../tools/sdcard.img");

#[cfg(feature = "ramdisk")]
pub fn checkout_fs_img() {
    let img_start = RAMDISK.as_ptr() as usize;
    let img_size = RAMDISK.len();
    println!("img_start: {:#x}, img_size: {:#x}", img_start, img_size);
}

fn init_block_device(blk: prob::DeviceInfo, mmio_transport: Option<MmioTransport>) {
    use drivers::block_device::VirtIOBlkWrapper;
    let (base_addr, irq) = (blk.base_addr, blk.irq);
    match blk.compatible.as_str() {
        "virtio,mmio" => {
            // qemu
            // let mut block_device = VirtIOBlkWrapper::new(blk.base_addr);
            let block_device = VirtIOBlkWrapper::from_mmio(mmio_transport.unwrap());
            println!("Init block device, base_addr:{:#x},irq:{}", base_addr, irq);
            let size = block_device.capacity();
            println!("Block device size is {}MB", size * 512 / 1024 / 1024);
            let block_device = Arc::new(GenericBlockDevice::new(Box::new(block_device)));
            block::init_block_device(block_device);
            // register_device(irq, block_device);
            println!("Init block device success");
        }
        "starfive,jh7110-sdio" => {
            // starfive2
            #[cfg(not(feature = "ramdisk"))]
            {
                use drivers::block_device::VF2SDDriver;
                let mut block_device = VF2SDDriver::new();
                block_device.init();
                let size = block_device.capacity();
                println!("Block device size is {}MB", size * 512 / 1024 / 1024);
                let block_device = Arc::new(GenericBlockDevice::new(Box::new(block_device)));
                block::init_block_device(block_device);
                // register_device(irq, block_device);
                println!("Init SDIO block device success");
            }
            #[cfg(feature = "ramdisk")]
            {
                init_ramdisk();
            }
        }
        name => {
            println!("Don't support block device: {}", name);
            #[cfg(feature = "ramdisk")]
            {
                init_ramdisk();
            }
            #[cfg(not(feature = "ramdisk"))]
            panic!("System need block device, but there is no block device");
        }
    }
}

#[cfg(feature = "ramdisk")]
fn init_ramdisk() {
    use drivers::block_device::MemoryFat32Img;
    checkout_fs_img();
    let data =
        unsafe { core::slice::from_raw_parts_mut(RAMDISK.as_ptr() as *mut u8, RAMDISK.len()) };
    let block_device = GenericBlockDevice::new(Box::new(MemoryFat32Img::new(data)));
    let block_device = Arc::new(block_device);
    block::init_block_device(block_device);
    println!("Init fake block device success");
}

fn init_gpu(gpu: prob::DeviceInfo, mmio_transport: Option<MmioTransport>) {
    let (base_addr, irq) = (gpu.base_addr, gpu.irq);
    println!("Init gpu, base_addr:{:#x},irq:{}", base_addr, irq);
    match gpu.compatible.as_str() {
        "virtio,mmio" => {
            // qemu
            use drivers::gpu::VirtIOGpuWrapper;
            let gpu = VirtIOGpuWrapper::from_mmio(mmio_transport.unwrap());
            let resolution = gpu.resolution();
            println!("GPU resolution: {:?}", resolution);
            let gpu = Arc::new(gpu);
            gpu::init_gpu(gpu);
            // let _ = register_device(irq, gpu);
            println!("Init gpu success");
        }
        name => {
            println!("Don't support gpu: {}", name);
        }
    }
}

fn init_input_device(input: prob::DeviceInfo, name: &str, mmio_transport: Option<MmioTransport>) {
    let (base_addr, irq) = (input.base_addr, input.irq);
    println!(
        "Init {} input device, base_addr:{:#x},irq:{}",
        name, base_addr, irq
    );
    match input.compatible.as_str() {
        "virtio,mmio" => {
            // qemu
            use drivers::input::VirtIOInputDriver;
            let input =
                VirtIOInputDriver::from_mmio(mmio_transport.unwrap(), MAX_INPUT_EVENT_NUM as u32);
            let input = Arc::new(input);
            match name {
                "mouse" => input::init_mouse_input_device(input.clone()),
                "keyboard" => input::init_keyboard_input_device(input.clone()),
                _ => panic!("Don't support {} input device", name),
            }
            let _ = register_device(irq, input);
            println!("Init keyboard input device success");
        }
        name => {
            println!("Don't support keyboard input device: {}", name);
        }
    }
}

fn init_net(_nic: Option<prob::DeviceInfo>) {
    // If we need run test, we should init loop device because no we can't route packet
    #[cfg(feature = "test")]
    {
        init_loop_device();
    }
    #[cfg(not(feature = "test"))]
    {
        let nic = _nic.unwrap();
        let (base_addr, irq) = (nic.base_addr, nic.irq);
        println!("Init net device, base_addr:{:#x},irq:{}", base_addr, irq);

        match nic.compatible.as_str() {
            "virtio,mmio" => {
                use core::str::FromStr;

                use config::{QEMU_GATEWAY, QEMU_IP};
                use drivers::net::{NetNeedFunc, VirtIONetDriver};
                use smoltcp::wire::IpAddress;
                let virtio_net = VirtIONetDriver::from_mmio(base_addr);
                let device = Box::new(virtio_net);
                netcore::init_net(
                    device,
                    Arc::new(NetNeedFunc),
                    IpAddress::from_str(QEMU_IP).unwrap(),
                    IpAddress::from_str(QEMU_GATEWAY).unwrap(),
                    true,
                );
                println!("Init net device success");
            }
            name => {
                panic!("Don't support net device: {}", name);
            }
        }
    }
}

#[cfg(feature = "test")]
fn init_loop_device() {
    use drivers::net::{LoopbackDev, NetNeedFunc};
    use smoltcp::wire::IpAddress;
    // use default ip and gateway for qemu
    let ip = IpAddress::v4(127, 0, 0, 1);
    let gate_way = IpAddress::v4(127, 0, 0, 1);
    let loopback = Box::new(LoopbackDev::new());
    netcore::init_net(loopback, Arc::new(NetNeedFunc), ip, gate_way, false);
    println!("Init net device success");
}
