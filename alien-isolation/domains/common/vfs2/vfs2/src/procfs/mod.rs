use alloc::{sync::Arc, vec::Vec};
use core::ffi::CStr;

use basic::println;
use interface::DomainType;
use shared_heap::DVec;
use vfscore::{dentry::VfsDentry, path::VfsPath};

use crate::shim::RootShimDentry;

///
/// ```bash
/// |
/// |-- meminfo
/// |-- interrupts
/// |-- mounts
/// |-- filesystems
/// ```
// todo!(use ramfs instead of dynfs)
pub fn init_procfs(root_dt: &Arc<dyn VfsDentry>) {
    let path = VfsPath::new(root_dt.clone(), root_dt.clone());
    let mut ramfs_ident = [0u8; 32];
    let ramfs_domain = basic::create_domain("ramfs", ramfs_ident.as_mut_slice()).unwrap();
    let name = CStr::from_bytes_until_nul(ramfs_ident.as_ref())
        .unwrap()
        .to_str()
        .unwrap();
    let name = Arc::new(Vec::from(name));
    let ramfs_root = match ramfs_domain {
        DomainType::FsDomain(ramfs) => {
            let mp = DVec::from_slice(b"/proc/self");
            let root_inode_id = ramfs.mount(&mp, None).unwrap();

            RootShimDentry::new(ramfs, root_inode_id, name)
        }
        _ => panic!("ramfs domain not create"),
    };
    path.join("self").unwrap().mount(ramfs_root, 0).unwrap();
    path.join("self/exe")
        .unwrap()
        .symlink("/bin/busybox")
        .unwrap();
    println!("procfs init success");
}
