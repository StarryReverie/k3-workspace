use alloc::sync::Arc;

use basic::constants::DeviceId;
use interface::CacheBlkDeviceDomain;
use shared_heap::DVec;
use vfscore::{
    error::VfsError,
    file::VfsFile,
    inode::{InodeAttr, VfsInode},
    utils::{VfsFileStat, VfsNodeType, VfsPollEvents},
    VfsResult,
};

pub struct BLKDevice {
    device_id: DeviceId,
    device: Arc<dyn CacheBlkDeviceDomain>,
}

impl BLKDevice {
    pub fn new(device_id: DeviceId, device: Arc<dyn CacheBlkDeviceDomain>) -> Self {
        Self { device_id, device }
    }
}

impl VfsFile for BLKDevice {
    fn read_at(&self, offset: u64, buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        let len = buf.len();
        let buf = self
            .device
            .read(offset, buf)
            .map_err(|_| VfsError::IoError)?;
        Ok((buf, len))
    }
    fn write_at(&self, offset: u64, buf: &DVec<u8>) -> VfsResult<usize> {
        self.device
            .write(offset, buf)
            .map_err(|_| VfsError::IoError)?;
        Ok(buf.len())
    }
    fn poll(&self, _event: VfsPollEvents) -> VfsResult<VfsPollEvents> {
        unimplemented!()
    }
    fn ioctl(&self, _cmd: u32, _arg: usize) -> VfsResult<usize> {
        unimplemented!()
    }
    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }
    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }
}

impl VfsInode for BLKDevice {
    fn set_attr(&self, _attr: InodeAttr) -> VfsResult<()> {
        Ok(())
    }
    fn get_attr(&self) -> VfsResult<VfsFileStat> {
        Ok(VfsFileStat {
            st_rdev: self.device_id.id(),
            st_size: self.device.get_capacity().unwrap(),
            st_blksize: 512,
            ..Default::default()
        })
    }
    fn inode_type(&self) -> VfsNodeType {
        VfsNodeType::BlockDevice
    }
}
