use downcast_rs::{impl_downcast, DowncastSync};
use gproxy::proxy;
use pconst::{epoll::EpollEvent, io::SeekFrom};
use shared_heap::{DBox, DVec};
use vfscore::utils::{VfsFileStat, VfsNodeType, VfsPollEvents};

use super::AlienResult;
use crate::{Basic, SocketID};
pub type InodeID = u64;
pub const VFS_ROOT_ID: InodeID = 0;
pub const VFS_STDIN_ID: InodeID = 1;
pub const VFS_STDOUT_ID: InodeID = 2;
pub const VFS_STDERR_ID: InodeID = 3;

pub struct DirEntryWrapper {
    /// ino is an inode number
    pub ino: u64,
    /// type is the file type
    pub ty: VfsNodeType,
    /// filename (null-terminated)
    pub name: [u8; 64],
    pub name_len: usize,
}

impl DirEntryWrapper {
    pub fn new(name: [u8; 64]) -> Self {
        Self {
            ino: 0,
            ty: VfsNodeType::Unknown,
            name,
            name_len: 0,
        }
    }
}
#[proxy(VfsDomainProxy,RwLock,Vec<u8>)]
pub trait VfsDomain: Basic + DowncastSync {
    fn init(&self, initrd: &[u8]) -> AlienResult<()>;
    fn vfs_poll(&self, inode: InodeID, events: VfsPollEvents) -> AlienResult<VfsPollEvents>;
    fn vfs_ioctl(&self, inode: InodeID, cmd: u32, arg: usize) -> AlienResult<usize>;
    fn vfs_open(
        &self,
        root: InodeID,
        path: &DVec<u8>,
        path_len: usize,
        mode: u32,
        open_flags: usize,
    ) -> AlienResult<InodeID>;
    fn vfs_close(&self, inode: InodeID) -> AlienResult<()>;
    fn vfs_getattr(
        &self,
        inode: InodeID,
        attr: DBox<VfsFileStat>,
    ) -> AlienResult<DBox<VfsFileStat>>;
    fn vfs_read_at(
        &self,
        inode: InodeID,
        offset: u64,
        buf: DVec<u8>,
    ) -> AlienResult<(DVec<u8>, usize)>;

    fn vfs_read(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;

    fn vfs_write_at(
        &self,
        inode: InodeID,
        offset: u64,
        buf: &DVec<u8>,
        w: usize,
    ) -> AlienResult<usize>;
    fn vfs_write(&self, inode: InodeID, buf: &DVec<u8>, w: usize) -> AlienResult<usize>;
    fn vfs_flush(&self, inode: InodeID) -> AlienResult<()>;
    fn vfs_fsync(&self, inode: InodeID) -> AlienResult<()>;
    fn vfs_lseek(&self, inode: InodeID, seek: SeekFrom) -> AlienResult<u64>;
    fn vfs_inode_type(&self, inode: InodeID) -> AlienResult<VfsNodeType>;
    fn vfs_readdir(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    fn vfs_get_path(&self, inode: InodeID, buf: DVec<u8>) -> AlienResult<(DVec<u8>, usize)>;
    /// truncate the file to len
    fn vfs_ftruncate(&self, inode: InodeID, len: u64) -> AlienResult<()>;
    fn vfs_update_atime(&self, inode: InodeID, atime_sec: u64, atime_nano: u64) -> AlienResult<()>;
    fn vfs_update_mtime(&self, inode: InodeID, mtime_sec: u64, mtime_nano: u64) -> AlienResult<()>;
    fn vfs_rename(
        &self,
        old_inode: InodeID,
        new_inode: InodeID,
        old_path: &DVec<u8>,
        old_len: usize,
        new_path: &DVec<u8>,
        new_len: usize,
        fs_info: (InodeID, InodeID),
        flag: u32,
    ) -> AlienResult<()>;
    fn vfs_unlink(
        &self,
        inode: InodeID,
        path: &DVec<u8>,
        path_len: usize,
        flag: u32,
    ) -> AlienResult<()>;
    fn do_fcntl(&self, inode: InodeID, cmd: usize, args: usize) -> AlienResult<isize>;
    fn do_pipe2(&self, flags: usize) -> AlienResult<(InodeID, InodeID)>;
    /// Create a socket and return the inode id
    fn do_socket(&self, socket_id: SocketID) -> AlienResult<InodeID>;
    /// Get the socket id from inode id
    fn socket_id(&self, inode: InodeID) -> AlienResult<SocketID>;
    fn do_poll_create(&self, flags: usize) -> AlienResult<InodeID>;
    fn do_poll_ctl(
        &self,
        inode: InodeID,
        op: u32,
        fd: usize,
        event: DBox<EpollEvent>,
    ) -> AlienResult<()>;
    fn do_eventfd(&self, init_val: u32, flags: u32) -> AlienResult<InodeID>;
}

impl_downcast!(sync VfsDomain);
