use alloc::sync::Arc;

use basic::{constants::DeviceId, sync::Mutex};
use spin::Lazy;
use storage::CustomStorge;
use vfs_common::id::DeviceIdManager;
use vfscore::utils::VfsNodeType;

type DeviceIdManagerType = Arc<Mutex<DeviceIdManager>, CustomStorge>;
static DEVICE_ID_MANAGER: Lazy<DeviceIdManagerType> = Lazy::new(|| {
    storage::get_or_insert("device_id_manager", || Mutex::new(DeviceIdManager::new()))
});
pub fn alloc_device_id(inode_type: VfsNodeType) -> DeviceId {
    DEVICE_ID_MANAGER.lock().alloc(inode_type)
}
