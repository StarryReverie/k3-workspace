use alloc::sync::Arc;

use basic::constants::DeviceId;
use interface::BufInputDomain;
use shared_heap::DVec;
use vfscore::{
    error::VfsError,
    file::VfsFile,
    inode::{InodeAttr, VfsInode},
    superblock::VfsSuperBlock,
    utils::{VfsFileStat, VfsNodeType, VfsPollEvents},
    VfsResult,
};

pub struct INPUTDevice {
    device_id: DeviceId,
    device: Arc<dyn BufInputDomain>,
}

impl INPUTDevice {
    pub fn new(device_id: DeviceId, device: Arc<dyn BufInputDomain>) -> Self {
        Self { device_id, device }
    }
}

impl VfsFile for INPUTDevice {
    fn read_at(&self, _offset: u64, mut buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        if buf.len() != 8 {
            return Err(VfsError::Invalid);
        }
        let event = self.device.event_block().unwrap();
        let event_bytes = event.to_be_bytes();
        buf.as_mut_slice().copy_from_slice(&event_bytes);
        Ok((buf, 1))
    }
    fn write_at(&self, _offset: u64, _buf: &DVec<u8>) -> VfsResult<usize> {
        Err(VfsError::Invalid)
    }
    fn poll(&self, event: VfsPollEvents) -> VfsResult<VfsPollEvents> {
        let mut res = VfsPollEvents::empty();
        if event.contains(VfsPollEvents::IN) && self.device.have_event().unwrap() {
            res |= VfsPollEvents::IN;
        }
        Ok(res)
    }
}

impl VfsInode for INPUTDevice {
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
