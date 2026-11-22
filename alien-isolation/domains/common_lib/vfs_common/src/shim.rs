use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::ffi::CStr;

use basic::sync::{Mutex, Once, OnceGet};
use interface::{DirEntryWrapper, DomainType, FsDomain, InodeID};
use shared_heap::{DBox, DVec};
use vfscore::{
    dentry::VfsDentry,
    error::VfsError,
    file::VfsFile,
    fstype::{FileSystemFlags, VfsFsType, VfsMountPoint},
    inode::{InodeAttr, VfsInode},
    superblock::{SuperType, VfsSuperBlock},
    utils::*,
    VfsResult,
};

static STATE_SAVE: Mutex<Vec<Arc<dyn VfsDentry>>> = Mutex::new(Vec::new());

pub fn insert_dentry_to_state(dentry: Arc<dyn VfsDentry>) {
    STATE_SAVE.lock().push(dentry);
}

pub struct RootShimDentry {
    fs_domain: Arc<dyn FsDomain>,
    inode_id: InodeID,
    inode: Arc<dyn VfsInode>,
    fs_domain_ident: Arc<Vec<u8>>,
    mount_point_this: Once<Weak<dyn VfsDentry>>,
    mount_point: Mutex<Option<VfsMountPoint>>,
    parent: Mutex<Option<Arc<dyn VfsDentry>>>,
    path: Mutex<String>,
    children: Mutex<BTreeMap<String, Arc<dyn VfsDentry>>>,
    name: Mutex<String>,
    #[allow(dead_code)]
    sb: Arc<dyn VfsSuperBlock>,
}

impl RootShimDentry {
    pub fn new(
        fs_domain: Arc<dyn FsDomain>,
        inode_id: InodeID,
        fs_domain_ident: Arc<Vec<u8>>,
    ) -> Arc<Self> {
        let fs = Arc::new(ShimFs::new(fs_domain.clone()));
        let inode = Arc::new(FsShimInode::new(
            fs_domain.clone(),
            inode_id,
            fs_domain_ident.clone(),
        ));
        let sb = Arc::new(ShimSuperBlock::new(fs_domain.clone(), inode.clone(), fs, fs_domain.fs_magic().unwrap()));
        inode.set_super_block(Arc::downgrade(&sb));
        let this = Arc::new(Self {
            fs_domain,
            inode_id,
            inode,
            fs_domain_ident,
            mount_point_this: Once::new(),
            mount_point: Mutex::new(None),
            parent: Mutex::new(None),
            path: Mutex::new(String::from("")),
            children: Mutex::new(BTreeMap::new()),
            name: Mutex::new(String::from("")),
            sb
        });
        let weak = Arc::downgrade(&(this.clone() as Arc<dyn VfsDentry>));
        this.mount_point_this.call_once(|| weak);
        this
    }

    pub fn from(inode: Arc<dyn VfsInode>) -> Arc<Self> {
        let inode = inode
            .downcast_arc::<FsShimInode>()
            .map_err(|_| VfsError::Invalid)
            .expect("inode is not FsShimInode");
        let inode_id = inode.ino;
        let fs_domain = inode.fs_domain.clone();
        let fs_domain_ident = inode.fs_domain_ident.clone();
        let sb = inode.sb.lock().as_ref().unwrap().clone();
        let shim_dentry = Arc::new(Self {
            fs_domain,
            inode_id,
            inode: inode as Arc<dyn VfsInode>,
            fs_domain_ident,
            mount_point_this: Once::new(),
            mount_point: Mutex::new(None),
            parent: Mutex::new(None),
            path: Mutex::new(String::from("")),
            children: Mutex::new(BTreeMap::new()),
            name: Mutex::new(String::from("")),
            sb: sb.upgrade().unwrap()
        });
        let weak = Arc::downgrade(&(shim_dentry.clone() as Arc<dyn VfsDentry>));
        shim_dentry.mount_point_this.call_once(|| weak);
        shim_dentry
    }
    pub fn inode_id(&self) -> InodeID {
        self.inode_id
    }

    pub fn fs_domain_ident(&self) -> Arc<Vec<u8>> {
        self.fs_domain_ident.clone()
    }

    pub fn fs_domain_ident_str(&self) -> &str {
        core::str::from_utf8(&self.fs_domain_ident).unwrap()
    }
}

impl Drop for FsShimInode {
    fn drop(&mut self) {
        self.fs_domain.drop_inode(self.ino).unwrap();
    }
}

impl VfsDentry for RootShimDentry {
    fn name(&self) -> String {
        if !self.name.lock().is_empty() {
            return self.name.lock().clone();
        }
        let buf = DVec::new_uninit(32);
        let (buf, l) = self.fs_domain.dentry_name(self.inode_id, buf).unwrap();
        let name = core::str::from_utf8(&buf.as_slice()[..l])
            .unwrap()
            .to_string();
        self.name.lock().clone_from(&name);
        name
    }

    fn to_mount_point(
        self: Arc<Self>,
        sub_fs_root: Arc<dyn VfsDentry>,
        _mount_flag: u32,
    ) -> VfsResult<()> {
        let dentry = sub_fs_root
            .downcast_arc::<RootShimDentry>()
            .map_err(|_| VfsError::Invalid)
            .expect("sub_fs_root is not RootShimDentry");
        let domain_ident = DVec::from_slice(&dentry.fs_domain_ident);
        let mount_inode_id = dentry.inode_id;
        self.fs_domain
            .dentry_to_mount_point(self.inode_id, &domain_ident, mount_inode_id)
            .unwrap();
        self.mount_point_this
            .call_once(|| Arc::downgrade(&(self.clone() as Arc<dyn VfsDentry>)));
        // let name = core::str::from_utf8(&domain_ident.as_slice())
        //     .unwrap();
        // println_color!(31,"<shim> to_mount_point for {:?}, mount_inode_id: {:?}, name: {:?}", self.inode_id, mount_inode_id, name);
        insert_dentry_to_state(dentry);
        Ok(())
    }

    fn inode(&self) -> VfsResult<Arc<dyn VfsInode>> {
        Ok(self.inode.clone())
    }

    fn mount_point(&self) -> Option<VfsMountPoint> {
        if let Some(mount_point) = self.mount_point.lock().as_ref() {
            return Some(mount_point.clone());
        }
        let domain_ident = DVec::new(0, 32);
        let (mount_point, inode_id) = self
            .fs_domain
            .dentry_mount_point(self.inode_id, domain_ident)
            .unwrap()?;
        let root_fs_domain_ident = CStr::from_bytes_until_nul(mount_point.as_slice())
            .unwrap()
            .to_str()
            .unwrap();
        let fs_domain = basic::get_domain(root_fs_domain_ident).unwrap();
        let fs_domain = match fs_domain {
            DomainType::FsDomain(fs_domain) => fs_domain,
            DomainType::DevFsDomain(fs_domain) => fs_domain,
            _ => panic!("mount_point domain is not FsDomain"),
        };
        let root_dentry = Self::new(
            fs_domain,
            inode_id,
            Arc::new(Vec::from(root_fs_domain_ident)),
        );
        let mount_point = VfsMountPoint {
            root: root_dentry,
            mount_point: self.mount_point_this.get_must().clone(),
            mnt_flags: 0,
        };
        self.mount_point.lock().replace(mount_point.clone());
        Some(mount_point)
    }

    fn clear_mount_point(&self) {
        self.fs_domain
            .dentry_clear_mount_point(self.inode_id)
            .unwrap();
        self.mount_point.lock().take();
    }

    fn find(&self, path: &str) -> Option<Arc<dyn VfsDentry>> {
        if let Some(dentry) = self.children.lock().get(path) {
            return Some(dentry.clone());
        }
        let shared_path = DVec::from_slice(path.as_bytes());
        let inode_id = self
            .fs_domain
            .dentry_find(self.inode_id, &shared_path)
            .unwrap()?;
        let this = Self::new(
            self.fs_domain.clone(),
            inode_id,
            self.fs_domain_ident.clone(),
        );
        Some(this)
    }

    fn insert(
        self: Arc<Self>,
        name: &str,
        child: Arc<dyn VfsInode>,
    ) -> VfsResult<Arc<dyn VfsDentry>> {
        let this = Self::from(child);
        self.children.lock().insert(name.to_string(), this.clone());
        Ok(this)
    }

    fn remove(&self, name: &str) -> Option<Arc<dyn VfsDentry>> {
        let shared_name = DVec::from_slice(name.as_bytes());
        self.fs_domain
            .dentry_remove(self.inode_id, &shared_name)
            .unwrap();
        self.children.lock().remove(name);
        // let inode = FsShimInode::new(self.fs_domain.clone(), inode_id, Arc::new(Vec::from(name)));
        // let shim_dentry = Self::from(Arc::new(inode));
        // Some(Arc::new(shim_dentry))
        None
    }

    fn parent(&self) -> Option<Arc<dyn VfsDentry>> {
        if let Some(parent) = self.parent.lock().as_ref() {
            return Some(parent.clone());
        }
        let parent_inode_id = self.fs_domain.dentry_parent(self.inode_id).unwrap()?;
        let dentry = Self::new(
            self.fs_domain.clone(),
            parent_inode_id,
            self.fs_domain_ident.clone(),
        );
        Some(dentry)
    }

    fn set_parent(&self, parent: &Arc<dyn VfsDentry>) {
        let parent = parent
            .clone()
            .downcast_arc::<RootShimDentry>()
            .map_err(|_| VfsError::Invalid)
            .expect("parent is not RootShimDentry");
        let domain_ident = DVec::from_slice(&parent.fs_domain_ident);
        let parent_inode_id = parent.inode_id;
        self.fs_domain
            .dentry_set_parent(self.inode_id, &domain_ident, parent_inode_id)
            .unwrap();
        self.parent.lock().replace(parent.clone());
        insert_dentry_to_state(parent);
    }

    fn path(&self) -> String {
        if !self.path.lock().is_empty() {
            return self.path.lock().clone();
        }
        let buf = DVec::new_uninit(64);
        let (buf, l) = self.fs_domain.dentry_path(self.inode_id, buf).unwrap();
        let path = core::str::from_utf8(&buf.as_slice()[..l])
            .unwrap()
            .to_string();
        self.path.lock().clone_from(&path);
        path
    }
}
pub struct FsShimInode {
    ino: InodeID,
    fs_domain: Arc<dyn FsDomain>,
    sb: Mutex<Option<Weak<dyn VfsSuperBlock>>>,
    fs_domain_ident: Arc<Vec<u8>>,
}

impl FsShimInode {
    pub fn new(fs_domain: Arc<dyn FsDomain>, ino: InodeID, fs_domain_ident: Arc<Vec<u8>>) -> Self {
        Self {
            fs_domain,
            ino,
            sb: Mutex::new(None),
            fs_domain_ident,
        }
    }
    pub fn set_super_block(&self, sb: Weak<ShimSuperBlock>) {
        *self.sb.lock() = Some(sb);
    }

    pub fn inode_id(&self) -> InodeID {
        self.ino
    }

    pub fn fs_domain(&self) -> Arc<dyn FsDomain> {
        self.fs_domain.clone()
    }

    pub fn clone_with_inode(&self, ino: InodeID) -> Self {
        Self {
            ino,
            fs_domain: self.fs_domain.clone(),
            sb: Mutex::new(self.sb.lock().clone()),
            fs_domain_ident: self.fs_domain_ident.clone(),
        }
    }

    pub fn fs_domain_ident(&self) -> Arc<Vec<u8>> {
        self.fs_domain_ident.clone()
    }
}

impl VfsFile for FsShimInode {
    fn read_at(&self, offset: u64, buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        let (shared_buf, len) = self.fs_domain.read_at(self.ino, offset, buf)?;
        Ok((shared_buf, len))
    }
    fn write_at(&self, offset: u64, buf: &DVec<u8>) -> VfsResult<usize> {
        let len = self.fs_domain.write_at(self.ino, offset, buf)?;
        Ok(len)
    }
    fn readdir(&self, start_index: usize) -> VfsResult<Option<VfsDirEntry>> {
        // todo!(fix name len)
        // let shared_name = [0; 64];
        let dir_entry = DBox::<DirEntryWrapper>::new_uninit();
        let dir_entry = self.fs_domain.readdir(self.ino, start_index, dir_entry)?;
        if dir_entry.name_len == 0 {
            Ok(None)
        } else {
            let name = core::str::from_utf8(&dir_entry.name.as_slice()[..dir_entry.name_len])
                .unwrap()
                .to_string();
            Ok(Some(VfsDirEntry {
                ino: dir_entry.ino,
                name,
                ty: dir_entry.ty,
            }))
        }
    }
    fn poll(&self, event: VfsPollEvents) -> VfsResult<VfsPollEvents> {
        let event = self.fs_domain.poll(self.ino, event)?;
        Ok(event)
    }
    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        let res = self.fs_domain.ioctl(self.ino, cmd, arg)?;
        Ok(res)
    }
    fn flush(&self) -> VfsResult<()> {
        self.fs_domain.flush(self.ino)?;
        Ok(())
    }
    fn fsync(&self) -> VfsResult<()> {
        self.fs_domain.fsync(self.ino)?;
        Ok(())
    }
}

impl VfsInode for FsShimInode {
    fn get_super_block(&self) -> VfsResult<Arc<dyn VfsSuperBlock>> {
        self.sb
            .lock()
            .as_ref()
            .unwrap()
            .upgrade()
            .ok_or(VfsError::Invalid)
    }
    fn node_perm(&self) -> VfsNodePerm {
        self.fs_domain.node_permission(self.ino).unwrap()
    }
    fn create(
        &self,
        name: &str,
        ty: VfsNodeType,
        perm: VfsNodePerm,
        rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn VfsInode>> {
        let shared_name = DVec::from_slice(name.as_bytes());
        let inode_id = self
            .fs_domain
            .create(self.ino, &shared_name, ty, perm, rdev)?;
        let inode = Arc::new(self.clone_with_inode(inode_id));
        Ok(inode)
    }
    fn link(&self, name: &str, src: Arc<dyn VfsInode>) -> VfsResult<Arc<dyn VfsInode>> {
        let shared_name = DVec::from_slice(name.as_bytes());
        let src = src
            .downcast_arc::<FsShimInode>()
            .map_err(|_| VfsError::Invalid)?;
        let inode_id = self.fs_domain.link(self.ino, &shared_name, src.ino)?;
        let inode = Arc::new(self.clone_with_inode(inode_id));
        Ok(inode)
    }
    fn unlink(&self, name: &str) -> VfsResult<()> {
        let shared_name = DVec::from_slice(name.as_bytes());
        self.fs_domain.unlink(self.ino, &shared_name)?;
        Ok(())
    }
    fn symlink(&self, name: &str, sy_name: &str) -> VfsResult<Arc<dyn VfsInode>> {
        let shared_name = DVec::from_slice(name.as_bytes());
        let shared_sy_name = DVec::from_slice(sy_name.as_bytes());
        let inode_id = self
            .fs_domain
            .symlink(self.ino, &shared_name, &shared_sy_name)?;
        let inode = Arc::new(self.clone_with_inode(inode_id));
        Ok(inode)
    }
    fn lookup(&self, name: &str) -> VfsResult<Arc<dyn VfsInode>> {
        let shared_name = DVec::from_slice(name.as_bytes());
        let inode_id = self.fs_domain.lookup(self.ino, &shared_name)?;
        let inode = Arc::new(self.clone_with_inode(inode_id));
        Ok(inode)
    }

    fn rmdir(&self, name: &str) -> VfsResult<()> {
        let shared_name = DVec::from_slice(name.as_bytes());
        self.fs_domain.rmdir(self.ino, &shared_name)?;
        Ok(())
    }
    fn readlink(&self, buf: DVec<u8>) -> VfsResult<(DVec<u8>, usize)> {
        let (shared_buf, len) = self.fs_domain.readlink(self.ino, buf)?;
        Ok((shared_buf, len))
    }
    fn set_attr(&self, attr: InodeAttr) -> VfsResult<()> {
        self.fs_domain.set_attr(self.ino, attr)?;
        Ok(())
    }
    fn get_attr(&self) -> VfsResult<VfsFileStat> {
        let attr = self.fs_domain.get_attr(self.ino)?;
        Ok(attr)
    }
    fn list_xattr(&self) -> VfsResult<Vec<String>> {
        panic!("We should not call this function now");
    }
    fn inode_type(&self) -> VfsNodeType {
        self.fs_domain.inode_type(self.ino).unwrap()
    }
    fn truncate(&self, len: u64) -> VfsResult<()> {
        self.fs_domain.truncate(self.ino, len)?;
        Ok(())
    }
    fn rename_to(
        &self,
        old_name: &str,
        new_parent: Arc<dyn VfsInode>,
        new_name: &str,
        flag: VfsRenameFlag,
    ) -> VfsResult<()> {
        let shared_old_name = DVec::from_slice(old_name.as_bytes());
        let shared_new_name = DVec::from_slice(new_name.as_bytes());
        let new_parent = new_parent
            .downcast_arc::<FsShimInode>()
            .map_err(|_| VfsError::Invalid)?;
        self.fs_domain.rename(
            self.ino,
            &shared_old_name,
            new_parent.ino,
            &shared_new_name,
            flag,
        )?;
        Ok(())
    }
    fn update_time(&self, time: VfsTime, now: VfsTimeSpec) -> VfsResult<()> {
        self.fs_domain.update_time(self.ino, time, now)?;
        Ok(())
    }
}
pub struct ShimSuperBlock {
    fs_domain: Arc<dyn FsDomain>,
    root_inode: Arc<dyn VfsInode>,
    fs: Arc<ShimFs>,
    magic:u128
}

impl ShimSuperBlock {
    pub fn new(
        fs_domain: Arc<dyn FsDomain>,
        root_inode: Arc<dyn VfsInode>,
        fs: Arc<ShimFs>,
        magic:u128,
    ) -> Self {
        Self {
            fs_domain,
            root_inode,
            fs,
            magic
        }
    }
}

impl VfsSuperBlock for ShimSuperBlock {
    fn sync_fs(&self, wait: bool) -> VfsResult<()> {
        self.fs_domain.sync_fs(wait)?;
        Ok(())
    }

    fn stat_fs(&self) -> VfsResult<VfsFsStat> {
        let fs_stat = DBox::<VfsFsStat>::new_uninit();
        let fs_stat = self.fs_domain.stat_fs(fs_stat)?;
        Ok(*fs_stat)
    }

    fn super_type(&self) -> SuperType {
        self.fs_domain.super_type().unwrap()
    }

    fn fs_type(&self) -> Arc<dyn VfsFsType> {
        self.fs.clone()
    }

    fn root_inode(&self) -> VfsResult<Arc<dyn VfsInode>> {
        Ok(self.root_inode.clone())
    }
    fn magic(&self) -> u128 {
        self.magic
    }
}
pub struct ShimFs {
    fs_domain: Arc<dyn FsDomain>,
}

impl ShimFs {
    pub fn new(fs_domain: Arc<dyn FsDomain>) -> Self {
        Self { fs_domain }
    }
}

impl VfsFsType for ShimFs {
    fn mount(
        self: Arc<Self>,
        _flags: u32,
        _ab_mnt: &str,
        _dev: Option<Arc<dyn VfsInode>>,
        _data: &[u8],
    ) -> VfsResult<Arc<dyn VfsDentry>> {
        panic!("We should call this function")
    }

    fn kill_sb(&self, _sb: Arc<dyn VfsSuperBlock>) -> VfsResult<()> {
        self.fs_domain.kill_sb()?;
        Ok(())
    }

    fn fs_flag(&self) -> FileSystemFlags {
        self.fs_domain.fs_flag().unwrap()
    }

    fn fs_name(&self) -> String {
        let buf = DVec::new_uninit(32);
        let (buf, len) = self.fs_domain.fs_name(buf).unwrap();
        core::str::from_utf8(&buf.as_slice()[..len])
            .unwrap()
            .to_string()
    }
}
