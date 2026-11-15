use alloc::format;
use core::cmp::min;

use config::CPU_NUM;
use vfscore::{file::VfsFile, inode::VfsInode, utils::VfsNodeType, VfsResult};

#[derive(Debug)]
pub struct CpuPossible;

impl VfsFile for CpuPossible {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = format!("0-{}", CPU_NUM - 1);
        if offset >= data.len() as u64 {
            return Ok(0);
        }
        let start = offset as usize;
        let copy_len = min(buf.len(), data.len() - start);
        buf[..copy_len].copy_from_slice(&data.as_bytes()[start..start + copy_len]);
        Ok(copy_len)
    }
}

impl VfsInode for CpuPossible {
    fn inode_type(&self) -> VfsNodeType {
        VfsNodeType::File
    }
}
