use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::fmt::Display;

use interface::DomainTypeRaw;

#[derive(Debug, Default)]
pub struct DomainInfo {
    pub ty_list: BTreeMap<DomainTypeRaw, Vec<DomainFileInfo>>,
    pub domain_list: BTreeMap<u64, DomainDataInfo>,
}

impl DomainInfo {
    pub fn new() -> Self {
        Self {
            ty_list: BTreeMap::new(),
            domain_list: BTreeMap::new(),
        }
    }
}

impl Display for DomainInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (ty, files) in self.ty_list.iter() {
            writeln!(f, "Domain type: {}", ty)?;
            for file in files.iter() {
                writeln!(f, "  - {}: {} bytes", file.name, file.size)?;
            }
        }
        for (id, data) in self.domain_list.iter() {
            writeln!(f, "Domain ID: {}", id)?;
            writeln!(f, "  - Name: {}", data.name)?;
            writeln!(f, "  - Type: {:?}", data.ty)?;
            writeln!(f, "  - Panic count: {}", data.panic_count)?;
            writeln!(f, "  - File: {}", data.file_info.name)?;
            writeln!(f, "  - Size: {} bytes", data.file_info.size)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DomainDataInfo {
    pub name: String,
    pub ty: DomainTypeRaw,
    pub panic_count: usize,
    pub file_info: DomainFileInfo,
}

#[derive(Debug, Clone)]
pub struct DomainFileInfo {
    pub name: String,
    pub size: usize,
}

impl DomainFileInfo {
    pub fn new(name: String, size: usize) -> Self {
        Self { name, size }
    }
    pub fn from((name, size): (String, usize)) -> Self {
        Self { name, size }
    }
}
