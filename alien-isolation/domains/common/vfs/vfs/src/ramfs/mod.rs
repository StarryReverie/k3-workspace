use alloc::sync::Arc;

use shared_heap::DVec;
use vfscore::{dentry::VfsDentry, utils::VfsNodeType, VfsResult};

///
/// ```bash
/// |
/// |-- root
///   |-- .bashrc
/// |--var
///   |-- log
///   |-- tmp(ramfs)
///   |-- run
/// |-- etc
///   |-- passwd
///   |--localtime
///   |--adjtime
/// |-- dev  (devfs)
/// |-- proc (procfs)
/// |-- sys  (sysfs)
/// |-- bin  (fat32)
/// |-- tmp   (ramfs)
/// ```
pub fn init_ramfs(root_dt: &Arc<dyn VfsDentry>) -> VfsResult<()> {
    let root_inode = root_dt.inode()?;
    let root = root_inode.create("root", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    let var = root_inode.create("var", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    var.create("log", VfsNodeType::Dir, "rwxrwxr-x".into(), None)?;
    var.create("tmp", VfsNodeType::Dir, "rwxrwxrwx".into(), None)?;
    var.create("run", VfsNodeType::Dir, "rwxrwxrwx".into(), None)?;
    let etc = root_inode.create("etc", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    let passwd = etc.create("passwd", VfsNodeType::File, "rw-r--r--".into(), None)?;
    let localtime = etc.create("localtime", VfsNodeType::File, "rw-r--r--".into(), None)?;
    let adjtime = etc.create("adjtime", VfsNodeType::File, "rw-r--r--".into(), None)?;

    let data = DVec::from_other_rvec_slice(b"root:x:0:0:root:/root:/bin/bash\n");
    passwd.write_at(0, &data)?;
    let utc = DVec::from_other_rvec_slice(UTC);
    localtime.write_at(0, &utc)?;
    let rtc_time = DVec::from_other_rvec_slice(RTC_TIME.as_bytes());
    adjtime.write_at(0, &rtc_time)?;

    root_inode.create("dev", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    root_inode.create("proc", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    root_inode.create("sys", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    root_inode.create("tmp", VfsNodeType::Dir, "rwxrwxrwx".into(), None)?;
    root_inode.create("tests", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;
    root_inode.create("domain", VfsNodeType::Dir, "rwxr-xr-x".into(), None)?;

    let _bashrc = root.create(".bashrc", VfsNodeType::File, "rwxrwxrwx".into(), None)?;

    basic::println!("ramfs init success");
    Ok(())
}

/// localtime文件中保存的内容
pub const UTC: &[u8] = &[
    b'T', b'Z', b'i', b'f', b'2', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1, 0, 0,
    0, 0x1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1, 0, 0, 0, 0x4, 0, 0, 0, 0, 0, 0, b'U', b'T', b'C',
    0, 0, 0, b'T', b'Z', b'i', b'f', b'2', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x1, 0, 0, 0, 0x1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1, 0, 0, 0, 0x4, 0, 0, 0, 0, 0, 0, b'U',
    b'T', b'C', 0, 0, 0, 0x0a, 0x55, 0x54, 0x43, 0x30, 0x0a,
];

/// rtc文件中保存的内容
pub const RTC_TIME: &str = r"
rtc_time	: 03:01:50
rtc_date	: 2023-07-11
alrm_time	: 13:03:24
alrm_date	: 2023-07-11
alarm_IRQ	: no
alrm_pending	: no
update IRQ enabled	: no
periodic IRQ enabled	: no
periodic IRQ frequency	: 1024
max user IRQ frequency	: 64
24hr		: yes
periodic_IRQ	: no
update_IRQ	: no
HPET_emulated	: no
BCD		: yes
DST_enable	: no
periodic_freq	: 1024
batt_status	: okay";
