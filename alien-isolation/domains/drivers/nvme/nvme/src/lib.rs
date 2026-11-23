#![no_std]
#![forbid(unsafe_code)]

mod nvme;
mod partition;

extern crate alloc;
use alloc::boxed::Box;
use core::{fmt::Debug, ops::Range};

use basic::{io::SafeIORegion, println, sync::Mutex, AlienError, AlienResult};
use interface::{define_unwind_for_BlkDeviceDomain, Basic, BlkDeviceDomain, DeviceBase};
use shared_heap::DVec;

use crate::{
    nvme::NvmeController,
    partition::{find_fat_partition, Partition},
};

struct Inner {
    controller: NvmeController,
    part: Partition,
}

pub struct NvmeDomain {
    inner: Mutex<Option<Inner>>,
}

impl Default for NvmeDomain {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl Debug for NvmeDomain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("NvmeDomain")
    }
}

impl DeviceBase for NvmeDomain {
    fn handle_irq(&self) -> AlienResult<()> {
        Ok(())
    }
}

impl Basic for NvmeDomain {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl BlkDeviceDomain for NvmeDomain {
    fn init(&self, device_info: &Range<usize>) -> AlienResult<()> {
        println!("nvme: bar {:#x}-{:#x}", device_info.start, device_info.end);
        let io = SafeIORegion::from(device_info.clone());
        let mut controller = match NvmeController::new(io) {
            Ok(controller) => controller,
            Err(error) => {
                println!("nvme: controller init failed: {}", error);
                return Ok(());
            }
        };

        match controller.identify() {
            Ok(identify) => identify.print(),
            Err(error) => {
                println!("nvme: identify failed: {}", error);
                return Ok(());
            }
        }
        if controller.block_size() != 512 {
            println!(
                "nvme: unsupported LBA size {}, keep block stack disabled",
                controller.block_size()
            );
            return Ok(());
        }

        let capacity = controller.blocks();
        let partition = {
            let ctrl = &mut controller;
            let mut read = |lba: u64, buf: &mut [u8]| ctrl.read_blocks(lba, buf).is_ok();
            find_fat_partition(&mut read, capacity)
        };
        let Some(part) = partition else {
            println!("nvme: no FAT partition found");
            return Ok(());
        };
        println!(
            "nvme: FAT partition lba={} blocks={} ({} MiB)",
            part.lba_offset,
            part.blocks,
            part.blocks * 512 / 1024 / 1024
        );
        *self.inner.lock() = Some(Inner { controller, part });
        Ok(())
    }

    fn read_block(&self, block: u32, mut data: DVec<u8>) -> AlienResult<DVec<u8>> {
        let mut guard = self.inner.lock();
        let Some(inner) = guard.as_mut() else {
            return Err(AlienError::EIO);
        };
        let lba = inner.part.lba_offset + block as u64;
        inner
            .controller
            .read_blocks(lba, data.as_mut_slice())
            .map_err(|_| AlienError::EIO)?;
        Ok(data)
    }

    fn write_block(&self, _block: u32, _data: &DVec<u8>) -> AlienResult<usize> {
        Err(AlienError::EROFS)
    }

    fn get_capacity(&self) -> AlienResult<u64> {
        Ok(self
            .inner
            .lock()
            .as_ref()
            .map(|inner| inner.part.blocks)
            .unwrap_or(0))
    }

    fn flush(&self) -> AlienResult<()> {
        Ok(())
    }
}

define_unwind_for_BlkDeviceDomain!(NvmeDomain);

pub fn main() -> Box<dyn BlkDeviceDomain> {
    Box::new(UnwindWrap::new(NvmeDomain::default()))
}
