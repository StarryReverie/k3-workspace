#![no_std]
#![forbid(unsafe_code)]

mod sdhci;

extern crate alloc;
use alloc::boxed::Box;
use core::{fmt::Debug, ops::Range};

use basic::{
    io::SafeIORegion,
    println,
    sync::Mutex,
    AlienError, AlienResult,
};
use interface::{define_unwind_for_BlkDeviceDomain, Basic, BlkDeviceDomain, DeviceBase};
use shared_heap::DVec;

use crate::sdhci::SdhciBlockDevice;

pub struct K3SdDomain {
    sd: Mutex<Option<SdhciBlockDevice>>,
}

impl Default for K3SdDomain {
    fn default() -> Self {
        Self {
            sd: Mutex::new(None),
        }
    }
}

impl Debug for K3SdDomain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("K3SdDomain")
    }
}

impl DeviceBase for K3SdDomain {
    fn handle_irq(&self) -> AlienResult<()> {
        Ok(())
    }
}

impl Basic for K3SdDomain {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl BlkDeviceDomain for K3SdDomain {
    fn init(&self, device_info: &Range<usize>) -> AlienResult<()> {
        println!("k3 sd: {:#x}-{:#x}", device_info.start, device_info.end);
        let mut device = SdhciBlockDevice::new(SafeIORegion::from(device_info.clone()));
        if device.init() {
            println!("k3 sd init success");
            *self.sd.lock() = Some(device);
        } else {
            // No card or no formatted FAT partition: keep booting without a
            // block device instead of failing domain loading.
            println!("k3 sd: no usable SD card");
        }
        Ok(())
    }

    fn read_block(&self, block: u32, mut data: DVec<u8>) -> AlienResult<DVec<u8>> {
        let mut guard = self.sd.lock();
        let Some(device) = guard.as_mut() else {
            return Err(AlienError::EIO);
        };
        device
            .read_block(block, data.as_mut_slice())
            .map_err(|_| AlienError::EIO)?;
        Ok(data)
    }

    fn write_block(&self, _block: u32, _data: &DVec<u8>) -> AlienResult<usize> {
        Err(AlienError::EROFS)
    }

    fn get_capacity(&self) -> AlienResult<u64> {
        Ok(self.sd.lock().as_ref().map(|device| device.capacity()).unwrap_or(0))
    }

    fn flush(&self) -> AlienResult<()> {
        Ok(())
    }
}

define_unwind_for_BlkDeviceDomain!(K3SdDomain);

pub fn main() -> Box<dyn BlkDeviceDomain> {
    Box::new(UnwindWrap::new(K3SdDomain::default()))
}
