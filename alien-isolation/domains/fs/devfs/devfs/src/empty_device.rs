use alloc::sync::Arc;

use basic::constants::DeviceId;
use interface::EmptyDeviceDomain;
use shared_heap::DVec;
use vfscore::{
    error::VfsError,
    file::VfsFile,
    inode::{InodeAttr, VfsInode},
    superblock::VfsSuperBlock,
    utils::{VfsFileStat, VfsNodePerm, VfsNodeType},
    VfsResult,
};

pub struct EmptyDevice {
    device_id: DeviceId,
    domain: Arc<dyn EmptyDeviceDomain>,
}
impl EmptyDevice {
    pub fn new(device_id: DeviceId, domain: Arc<dyn EmptyDeviceDomain>) -> Self {
        Self { device_id, domain }
    }
}

impl VfsFile for EmptyDevice {
    fn read_at(&self, _offset: u64, buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        let shared_buf = self.domain.read(buf)?;
        let len = shared_buf.len();
        Ok((shared_buf, len))
    }
    fn write_at(&self, _offset: u64, buf: &DVec<u8>) -> VfsResult<usize> {
        Ok(buf.len())
    }
}

impl VfsInode for EmptyDevice {
    fn get_super_block(&self) -> VfsResult<Arc<dyn VfsSuperBlock>> {
        Err(VfsError::NoSys)
    }

    fn node_perm(&self) -> VfsNodePerm {
        VfsNodePerm::empty()
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
