use alloc::string::ToString;
use core::fmt::Debug;

use basic::AlienResult;
use generic::GenericFsDomain;
use interface::*;
use shared_heap::{DBox, DVec};
use vfscore::{fstype::FileSystemFlags, inode::InodeAttr, superblock::SuperType, utils::*};

use crate::{DEV_MAP, TASK_DOMAIN};

#[derive(Debug)]
pub struct DevFsDomainImpl {
    generic_fs: GenericFsDomain,
}

impl DevFsDomainImpl {
    pub fn new(generic_fs_domain: GenericFsDomain) -> Self {
        Self {
            generic_fs: generic_fs_domain,
        }
    }
}

impl FsDomain for DevFsDomainImpl {
    fn init(&self) -> AlienResult<()> {
        let task_domain = basic::get_domain("task").unwrap();
        match task_domain {
            DomainType::TaskDomain(task) => {
                TASK_DOMAIN.call_once(|| task);
            }
            _ => panic!("task domain not found"),
        }
        self.generic_fs.init()
    }

    fn mount(&self, mp: &DVec<u8>, dev_inode: Option<DBox<MountInfo>>) -> AlienResult<InodeID> {
        self.generic_fs.mount(mp, dev_inode)
    }

    fn root_inode_id(&self) -> AlienResult<InodeID> {
        self.generic_fs.root_inode_id()
    }
    fn drop_inode(&self, inode: InodeID) -> AlienResult<()> {
        self.generic_fs.drop_inode(inode)
    }

    fn dentry_name(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        self.generic_fs.dentry_name(inode, buf)
    }

    fn dentry_path(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        self.generic_fs.dentry_path(inode, buf)
    }
    fn dentry_set_parent(
        &self,
        inode: InodeID,
        domain_ident: &DVec<u8>,
        parent: InodeID,
    ) -> AlienResult<()> {
        self.generic_fs
            .dentry_set_parent(inode, domain_ident, parent)
    }

    fn dentry_parent(&self, inode: InodeID) -> AlienResult<Option<InodeID>> {
        self.generic_fs.dentry_parent(inode)
    }

    fn dentry_to_mount_point(
        &self,
        inode: InodeID,
        domain_ident: &DVec<u8>,
        mount_inode_id: InodeID,
    ) -> AlienResult<()> {
        self.generic_fs
            .dentry_to_mount_point(inode, domain_ident, mount_inode_id)
    }

    fn dentry_mount_point(
        &self,
        inode: InodeID,
        domain_ident: DVec<u8>,
    ) -> AlienResult<Option<(DVec<u8>, InodeID)>> {
        self.generic_fs.dentry_mount_point(inode, domain_ident)
    }

    fn dentry_clear_mount_point(&self, inode: InodeID) -> AlienResult<()> {
        self.generic_fs.dentry_clear_mount_point(inode)
    }

    fn dentry_find(&self, inode: InodeID, name: &DVec<u8>) -> AlienResult<Option<InodeID>> {
        self.generic_fs.dentry_find(inode, name)
    }

    fn dentry_remove(&self, inode: InodeID, name: &DVec<u8>) -> AlienResult<()> {
        self.generic_fs.dentry_remove(inode, name)
    }

    fn read_at(
        &self,
        inode: InodeID,
        offset: u64,
        buf: DVec<u8>,
    ) -> AlienResult<(DVec<u8>, usize)> {
        self.generic_fs.read_at(inode, offset, buf)
    }

    fn write_at(&self, inode: InodeID, offset: u64, buf: &DVec<u8>) -> AlienResult<usize> {
        self.generic_fs.write_at(inode, offset, buf)
    }

    fn readdir(
        &self,
        inode: InodeID,
        start_index: usize,
        entry: DBox<DirEntryWrapper>,
    ) -> AlienResult<DBox<DirEntryWrapper>> {
        self.generic_fs.readdir(inode, start_index, entry)
    }

    fn poll(&self, inode: InodeID, mask: VfsPollEvents) -> AlienResult<VfsPollEvents> {
        self.generic_fs.poll(inode, mask)
    }

    fn ioctl(&self, inode: InodeID, cmd: u32, arg: usize) -> AlienResult<usize> {
        self.generic_fs.ioctl(inode, cmd, arg)
    }

    fn flush(&self, inode: InodeID) -> AlienResult<()> {
        self.generic_fs.flush(inode)
    }

    fn fsync(&self, inode: InodeID) -> AlienResult<()> {
        self.generic_fs.fsync(inode)
    }

    fn rmdir(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<()> {
        self.generic_fs.rmdir(parent, name)
    }

    fn node_permission(&self, inode: InodeID) -> AlienResult<VfsNodePerm> {
        self.generic_fs.node_permission(inode)
    }

    fn create(
        &self,
        parent: InodeID,
        name: &DVec<u8>,
        ty: VfsNodeType,
        perm: VfsNodePerm,
        rdev: Option<u64>,
    ) -> AlienResult<InodeID> {
        self.generic_fs.create(parent, name, ty, perm, rdev)
    }

    fn link(&self, parent: InodeID, name: &DVec<u8>, src: InodeID) -> AlienResult<InodeID> {
        self.generic_fs.link(parent, name, src)
    }

    fn unlink(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<()> {
        self.generic_fs.unlink(parent, name)
    }

    fn symlink(&self, parent: InodeID, name: &DVec<u8>, link: &DVec<u8>) -> AlienResult<InodeID> {
        self.generic_fs.symlink(parent, name, link)
    }

    fn lookup(&self, parent: InodeID, name: &DVec<u8>) -> AlienResult<InodeID> {
        self.generic_fs.lookup(parent, name)
    }

    fn readlink(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        self.generic_fs.readlink(inode, buf)
    }

    fn set_attr(&self, inode: InodeID, attr: InodeAttr) -> AlienResult<()> {
        self.generic_fs.set_attr(inode, attr)
    }

    fn get_attr(&self, inode: InodeID) -> AlienResult<VfsFileStat> {
        self.generic_fs.get_attr(inode)
    }

    fn inode_type(&self, inode: InodeID) -> AlienResult<VfsNodeType> {
        self.generic_fs.inode_type(inode)
    }

    fn truncate(&self, inode: InodeID, len: u64) -> AlienResult<()> {
        self.generic_fs.truncate(inode, len)
    }

    fn rename(
        &self,
        old_parent: InodeID,
        old_name: &DVec<u8>,
        new_parent: InodeID,
        new_name: &DVec<u8>,
        flags: VfsRenameFlag,
    ) -> AlienResult<()> {
        self.generic_fs
            .rename(old_parent, old_name, new_parent, new_name, flags)
    }

    fn update_time(&self, inode: InodeID, time: VfsTime, now: VfsTimeSpec) -> AlienResult<()> {
        self.generic_fs.update_time(inode, time, now)
    }

    fn sync_fs(&self, wait: bool) -> AlienResult<()> {
        self.generic_fs.sync_fs(wait)
    }

    fn stat_fs(&self, fs_stat: DBox<VfsFsStat>) -> AlienResult<DBox<VfsFsStat>> {
        self.generic_fs.stat_fs(fs_stat)
    }

    fn super_type(&self) -> AlienResult<SuperType> {
        self.generic_fs.super_type()
    }

    fn kill_sb(&self) -> AlienResult<()> {
        self.generic_fs.kill_sb()
    }

    fn fs_flag(&self) -> AlienResult<FileSystemFlags> {
        self.generic_fs.fs_flag()
    }
    fn fs_name(&self, name: DVec<u8>) -> AlienResult<(DVec<u8>, usize)> {
        self.generic_fs.fs_name(name)
    }
    fn fs_magic(&self) -> AlienResult<u128> {
        self.generic_fs.fs_magic()
    }
}

impl Basic for DevFsDomainImpl {
    fn domain_id(&self) -> u64 {
        shared_heap::domain_id()
    }
}

impl DevFsDomain for DevFsDomainImpl {
    fn register(&self, rdev: u64, device_domain_name: &DVec<u8>) -> AlienResult<()> {
        let name = core::str::from_utf8(device_domain_name.as_slice()).unwrap();
        let mut map = DEV_MAP.lock();
        map.insert(rdev, name.to_string());
        Ok(())
    }
}
impl Basic for UnwindWrap {
    fn domain_id(&self) -> u64 {
        self.0.domain_id()
    }
}

define_unwind_for_DevFsDomain!(DevFsDomainImpl);
impl_unwind_for_FsDomain!(UnwindWrap);
