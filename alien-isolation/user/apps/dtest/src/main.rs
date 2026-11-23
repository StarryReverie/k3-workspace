#![no_std]
#![no_main]

use domain_helper::DomainHelperBuilder;
use Mstd::{
    domain::{update_domain, DomainTypeRaw},
    fs::{close, open, read, OpenFlags},
    println,
};

/// Replace the live `shadow_blk` domain with `sshadow_blk` and verify that
/// the mounted FAT is still readable through the replacement.
#[no_mangle]
fn main() -> isize {
    println!("SWAP_BEGIN");

    // The TCB cannot replace an FsDomain. This must come back as an error;
    // before the hardening it panicked and took the whole system down.
    if update_domain("ramfs-1", "sshadow_blk", DomainTypeRaw::FsDomain) == 0 {
        println!("SWAP_FAIL unsupported replace accepted");
        return -1;
    }
    println!("SWAP_REJECT_OK");

    let builder = DomainHelperBuilder::new()
        .ty(DomainTypeRaw::ShadowBlockDomain)
        .domain_file_path("/tests/gsshadow_blk\0")
        .domain_file_name("sshadow_blk")
        .domain_name("shadow_blk-1");
    if let Err(e) = builder.clone().register_domain_file() {
        println!("SWAP_FAIL register: {}", e);
        return -1;
    }
    if let Err(e) = builder.update_domain() {
        println!("SWAP_FAIL update: {}", e);
        return -1;
    }

    let fd = open("/tests/hello.txt\0", OpenFlags::O_RDONLY);
    if fd < 0 {
        println!("SWAP_FAIL open /tests/hello.txt");
        return -1;
    }
    let mut buf = [0u8; 64];
    let n = read(fd as usize, &mut buf);
    close(fd as _);
    if n <= 0 {
        println!("SWAP_FAIL read /tests/hello.txt");
        return -1;
    }
    let text = core::str::from_utf8(&buf[..n as usize]).unwrap_or("");
    if !text.starts_with("alien isolation block read ok") {
        println!("SWAP_FAIL unexpected content: {}", text);
        return -1;
    }
    println!("SWAP_OK");
    0
}
