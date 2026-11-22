use alloc::{format, sync::Arc};
use core::{cmp::min, ops::Deref};

use basic::constants::{
    io::{RtcTime, TeletypeCommand},
    DeviceId,
};
use interface::{RtcDomain, TaskDomain};
use pod::Pod;
use shared_heap::{DBox, DVec};
use vfscore::{
    error::VfsError,
    file::VfsFile,
    inode::{InodeAttr, VfsInode},
    superblock::VfsSuperBlock,
    utils::{VfsFileStat, VfsNodeType},
    VfsResult,
};

pub struct RTCDevice {
    device_id: DeviceId,
    device: Arc<dyn RtcDomain>,
    task_domain: Arc<dyn TaskDomain>,
}

impl RTCDevice {
    pub fn new(device_id: DeviceId, device: Arc<dyn RtcDomain>, task: Arc<dyn TaskDomain>) -> Self {
        Self {
            device_id,
            device,
            task_domain: task,
        }
    }
}

impl VfsFile for RTCDevice {
    fn read_at(&self, _offset: u64, mut buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        let mut time = DBox::new(RtcTime::default());
        time = self.device.read_time(time).unwrap();
        let str = format!("{:?}", time.deref());
        let bytes = str.as_bytes();
        let min_len = min(buf.len(), bytes.len());
        buf.as_mut_slice()[..min_len].copy_from_slice(&bytes[..min_len]);
        Ok((buf, min_len))
    }
    fn write_at(&self, _offset: u64, _buf: &DVec<u8>) -> VfsResult<usize> {
        todo!()
    }
    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        let cmd = TeletypeCommand::try_from(cmd).map_err(|_| VfsError::Invalid)?;
        match cmd {
            TeletypeCommand::RTC_RD_TIME => {
                let mut time = DBox::new(RtcTime::default());
                time = self.device.read_time(time).unwrap();
                self.task_domain
                    .copy_to_user(arg, time.deref().as_bytes())
                    .unwrap();
            }
            _ => return Err(VfsError::Invalid),
        }
        Ok(0)
    }
    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }
    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }
}

impl VfsInode for RTCDevice {
    fn get_super_block(&self) -> VfsResult<Arc<dyn VfsSuperBlock>> {
        Err(VfsError::NoSys)
    }

    fn set_attr(&self, _attr: InodeAttr) -> VfsResult<()> {
        Ok(())
    }

    fn get_attr(&self) -> VfsResult<VfsFileStat> {
        Ok(VfsFileStat {
            st_rdev: self.device_id.id(),
            ..Default::default()
        })
    }

    fn inode_type(&self) -> VfsNodeType {
        VfsNodeType::CharDevice
    }
}
