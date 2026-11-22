use alloc::collections::BTreeMap;

use basic::constants::DeviceId;
use vfscore::utils::VfsNodeType;

pub struct DeviceIdManager {
    map: BTreeMap<u32, u32>,
}

impl Default for DeviceIdManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceIdManager {
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
    pub fn alloc(&mut self, inode_type: VfsNodeType) -> DeviceId {
        assert!(matches!(
            inode_type,
            VfsNodeType::CharDevice | VfsNodeType::BlockDevice
        ));
        let id = self.map.entry(inode_type.to_u32()).or_insert(0);
        *id += 1;
        DeviceId::new(inode_type.to_u32(), *id)
    }
}
pub trait InodeType2u32 {
    fn to_u32(&self) -> u32;
}

impl InodeType2u32 for VfsNodeType {
    fn to_u32(&self) -> u32 {
        match self {
            VfsNodeType::CharDevice => 2,
            VfsNodeType::BlockDevice => 3,
            _ => 0,
        }
    }
}
