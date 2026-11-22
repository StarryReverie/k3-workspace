use alloc::{sync::Arc, vec::Vec};
use core::fmt::Debug;

use basic::constants::io::OpenFlags;
use interface::FsDomain;
use storage::CustomStorge;

#[derive(Debug)]
pub struct KernelFileMeta {
    pub pos: u64,
    pub open_flag: OpenFlags,
    pub real_inode_id: u64,
    pub fs_domain: Arc<dyn FsDomain>,
    pub fs_domain_ident: Vec<u8, CustomStorge>,
}

impl KernelFileMeta {
    pub fn new(
        pos: u64,
        open_flag: OpenFlags,
        real_inode_id: u64,
        fs_domain: Arc<dyn FsDomain>,
        fs_domain_ident: Vec<u8, CustomStorge>,
    ) -> Self {
        Self {
            pos,
            open_flag,
            real_inode_id,
            fs_domain,
            fs_domain_ident,
        }
    }
}

impl Drop for KernelFileMeta {
    fn drop(&mut self) {
        log::info!("drop KernelFileMeta");
    }
}
