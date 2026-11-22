use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use shared_heap::{DBox, DVec};
use vfscore::{fstype::FileSystemFlags, inode::InodeAttr, superblock::SuperType, utils::*};

use super::AlienResult;
use crate::{Basic, DirEntryWrapper, InodeID};

#[proxy(FsDomainProxy, RwLock)]
pub trait FsDomain: Basic + DowncastSync {
    fn init(&self) -> AlienResult<()>;
    fn mount(&self, mp: &DVec<u8>, dev_inode: Option<DBox<MountInfo>>) -> AlienResult<InodeID>;
    fn root_inode_id(&self) -> AlienResult<InodeID>;
    fn drop_inode(&self, inode: InodeID) -> AlienResult<()>;

    fn dentry_name(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    fn dentry_path(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    /// The domain_ident is the domain name of the parent fs domain
    fn dentry_set_parent(
        &self,
        inode: InodeID,
        domain_ident: &DVec<u8>,
        parent: InodeID,
    ) -> AlienResult<()>;
    fn dentry_parent(&self, inode: InodeID) -> AlienResult<Option<InodeID>>;

    fn dentry_to_mount_point(
        &self,
        inode: InodeID,
        domain_ident: &DVec<u8>,
        mount_inode_id: InodeID,
    ) -> AlienResult<()>;
    fn dentry_mount_point(
        &self,
        inode: InodeID,
        domain_ident: DVec<u8>,
    ) -> AlienResult<Option<(DVec<u8>, InodeID)>>;
    fn dentry_clear_mount_point(&self, inode: InodeID) -> AlienResult<()>;
    fn dentry_find(&self, inode: InodeID, name: &DVec<u8>) -> AlienResult<Option<InodeID>>;
    fn dentry_remove(&self, inode: InodeID, name: &DVec<u8>) -> AlienResult<()>;

    // file operations
    fn read_at(&self, inode: InodeID, offset: u64, buf: DVec<u8>)
        -> AlienResult<(DVec<u8>, usize)>;
    fn write_at(&self, inode: InodeID, offset: u64, buf: &DVec<u8>) -> AlienResult<usize>;
    fn readdir(
        &self,
        inode: InodeID,
        start_index: usize,
        entry: DBox<DirEntryWrapper>,
    ) -> AlienResult<DBox<DirEntryWrapper>>;
    fn poll(&self, inode: InodeID, mask: VfsPollEvents) -> AlienResult<VfsPollEvents>;
    fn ioctl(&self, inode: InodeID, cmd: u32, arg: usize) -> AlienResult<usize>;
    fn flush(&self, inode: InodeID) -> AlienResult<()>;
    fn fsync(&self, inode: InodeID) -> AlienResult<()>;

    // inode operations
    fn rmdir(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<()>;
    fn node_permission(&self, inode: InodeID) -> AlienResult<VfsNodePerm>;
    fn create(
        &self,
        parent: InodeID,
        name: &DVec<u8>,
        ty: VfsNodeType,
        perm: VfsNodePerm,
        rdev: Option<u64>,
    ) -> AlienResult<InodeID>;
    fn link(&self, parent: InodeID, name: &DVec<u8>, src: InodeID) -> AlienResult<InodeID>;
    fn unlink(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<()>;
    fn symlink(&self, parent: InodeID, name: &DVec<u8>, link: &DVec<u8>) -> AlienResult<InodeID>;
    fn lookup(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<InodeID>;
    fn readlink(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    fn set_attr(&self, inode: InodeID, attr: InodeAttr) -> AlienResult<()>;
    fn get_attr(&self, inode: InodeID) -> AlienResult<VfsFileStat>;
    fn inode_type(&self, inode: InodeID) -> AlienResult<VfsNodeType>;
    fn truncate(&self, inode: InodeID, len: u64) -> AlienResult<()>;
    fn rename(
        &self,
        old_parent: InodeID,
        old_name: &DVec<u8>,
        new_parent: InodeID,
        new_name: &DVec<u8>,
        flags: VfsRenameFlag,
    ) -> AlienResult<()>;
    fn update_time(&self, inode: InodeID, time: VfsTime, now: VfsTimeSpec) -> AlienResult<()>;

    // superblock operations
    fn sync_fs(&self, wait: bool) -> AlienResult<()>;
    fn stat_fs(&self, fs_stat: DBox<VfsFsStat>) -> AlienResult<DBox<VfsFsStat>>;
    fn super_type(&self) -> AlienResult<SuperType>;

    // fs
    fn kill_sb(&self) -> AlienResult<()>;
    fn fs_flag(&self) -> AlienResult<FileSystemFlags>;
    fn fs_name(&self, name: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    fn fs_magic(&self) -> AlienResult<u128>;
}

impl_downcast!(sync FsDomain);

#[proxy(DevFsDomainProxy, RwLock)]
pub trait DevFsDomain: FsDomain + DowncastSync {
    fn register(&self, rdev: u64, device_domain_name: &DVec<u8>) -> AlienResult<()>;
}

impl_downcast!(sync DevFsDomain);

pub struct MountInfo {
    pub mount_inode_id: InodeID,
    pub domain_ident: [u8; 32],
}
