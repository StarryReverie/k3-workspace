use alloc::{string::String, sync::Arc, vec, vec::Vec};
use constants::AlienResult;
use core2::io::Read;
use cpio_reader::Mode;
use vfscore::{
    dentry::VfsDentry,
    path::VfsPath,
    utils::VfsInodeMode,
};

pub fn populate_initrd(root: Arc<dyn VfsDentry>) -> AlienResult<()> {
    parse_initrd_data(root)?;
    println!("Initrd populate success");
    Ok(())
}

fn ensure_parent_dirs(root: &Arc<dyn VfsDentry>, name: &str) -> AlienResult<()> {
    let parts: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 1 {
        return Ok(());
    }
    let mut current = String::new();
    for part in &parts[..parts.len() - 1] {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        let path = VfsPath::new(root.clone(), root.clone()).join(&current)?;
        path.open(Some(VfsInodeMode::from_bits_truncate(0o040755)))?;
    }
    Ok(())
}

fn parse_initrd_data(root: Arc<dyn VfsDentry>) -> AlienResult<()> {
    let mut guard = mem::data::INITRD_DATA.lock();
    if guard.is_some() {
        let path = VfsPath::new(root.clone(), root.clone());
        let data = guard.as_ref().unwrap();
        let st = data.data_ptr;
        let size = data.size;
        let data = unsafe { core::slice::from_raw_parts(st as *const u8, size) };
        let mut decoder = libflate::gzip::Decoder::new(data).unwrap();
        let mut buf = vec![];
        let _r = decoder.read_to_end(&mut buf).unwrap();
        for entry in cpio_reader::iter_files(&buf) {
            let name = entry
                .name()
                .trim_start_matches("./")
                .trim_start_matches('/')
                .trim_end_matches('/');
            if name.is_empty() || name == "." || name == "TRAILER!!!" {
                continue;
            }
            ensure_parent_dirs(&root, name)?;
            let mode = entry.mode();
            let target = path.join(name)?;
            if mode.contains(Mode::DIRECTORY) {
                target.open(Some(VfsInodeMode::from_bits_truncate(0o040755)))?;
            } else if mode.contains(Mode::SYMBOLIK_LINK) {
                let data = entry.file();
                let link = core::str::from_utf8(data).unwrap();
                target.symlink(link)?;
            } else if mode.contains(Mode::REGULAR_FILE) {
                let inode_mode = VfsInodeMode::from_bits_truncate(mode.bits());
                let f = target.open(Some(inode_mode))?;
                f.inode()?.write_at(0, entry.file())?;
            }
        }
        // release the page frame
        guard.take();
    }
    Ok(())
}
