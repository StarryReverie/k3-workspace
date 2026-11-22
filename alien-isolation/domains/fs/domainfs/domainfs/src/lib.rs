#![no_std]
#![forbid(unsafe_code)]

mod custom_inode;
mod domain_info;

extern crate alloc;
use alloc::{boxed::Box, string::ToString, sync::Arc};

use basic::{
    println,
    sync::{Mutex, Once},
    DomainInfoSet,
};
use custom_fs::FsKernelProvider;
use generic::{GenericFsDomain, UnwindWrap};
use interface::FsDomain;
use vfscore::utils::VfsTimeSpec;

use crate::domain_info::domain_fs_root;

#[derive(Clone)]
pub struct CommonFsProviderImpl;

impl FsKernelProvider for CommonFsProviderImpl {
    fn current_time(&self) -> VfsTimeSpec {
        VfsTimeSpec::new(0, 0)
    }
}

type CustomFs = custom_fs::CustomFs<CommonFsProviderImpl, Mutex<()>>;

type DomainFsDomain = GenericFsDomain;

pub fn main() -> Box<dyn FsDomain> {
    let root = domain_fs_root();
    let domain_fs = Arc::new(CustomFs::new(
        CommonFsProviderImpl,
        "domainfs",
        root.clone(),
    ));
    let magic = domain_fs.magic();

    root.set_magic(magic);
    Box::new(UnwindWrap::new(DomainFsDomain::new(
        domain_fs,
        "domainfs".to_string(),
        None,
        Some(init),
    )))
}

static DOMAIN_INFO: Once<Arc<DomainInfoSet>> = Once::new();
fn init() {
    let info = basic::domain_info();
    DOMAIN_INFO.call_once(|| info);
    println!("get domain info success");
}
