#![no_std]

mod vm;

extern crate alloc;
#[macro_use]
extern crate log;
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::{
    fmt::{Debug, Formatter},
    ops::Range,
};

use log::{debug, trace};
use memory_addr::VirtAddr;
use storage::StorageArg;
pub use vm::{DomainArea, DomainVmOps};
use xmas_elf::{program::Type, sections::SectionData, ElfFile};

use crate::vm::DomainMappingFlags;
const FRAME_SIZE: usize = 4096;
type Result<T> = core::result::Result<T, &'static str>;

pub struct DomainLoader<V: DomainVmOps> {
    entry_point: usize,
    data: Arc<Vec<u8>>,
    virt_start: usize,
    module_area: Option<Box<dyn DomainArea>>,
    ident: String,
    text_section: Range<usize>,
    _phantom: core::marker::PhantomData<V>,
}

impl<V: DomainVmOps> Debug for DomainLoader<V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DomainLoader")
            .field("entry", &self.entry_point)
            .field("phy_start", &self.virt_start)
            .field("ident", &self.ident)
            .field("text_section", &self.text_section)
            .finish()
    }
}

impl<V: DomainVmOps> Clone for DomainLoader<V> {
    fn clone(&self) -> Self {
        Self {
            entry_point: 0,
            data: self.data.clone(),
            virt_start: 0,
            ident: self.ident.to_string(),
            module_area: None,
            text_section: self.text_section.clone(),
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<V: DomainVmOps> DomainLoader<V> {
    pub fn new(data: Arc<Vec<u8>>, ident: &str) -> Self {
        Self {
            entry_point: 0,
            data,
            virt_start: 0,
            ident: ident.to_string(),
            module_area: None,
            text_section: 0..0,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Return the domain file info(name, size)
    pub fn domain_file_info(&self) -> (String, usize) {
        (self.ident.clone(), self.data.len())
    }

    pub fn empty() -> Self {
        Self::new(Arc::new(vec![]), "empty_loader")
    }

    fn entry_point(&self) -> usize {
        self.entry_point
    }

    pub fn call<T: ?Sized, C: 'static + ?Sized, F>(
        &self,
        id: u64,
        use_old_id: Option<u64>,
        callback: F,
    ) -> Box<T>
    where
        F: FnOnce(
            Option<u64>,
        ) -> (
            &'static C,
            &'static dyn shared_heap::SharedHeapAlloc,
            StorageArg,
        ),
    {
        type F<T, C> =
            fn(&'static C, u64, &'static dyn shared_heap::SharedHeapAlloc, StorageArg) -> Box<T>;
        let main =
            unsafe { core::mem::transmute::<*const (), F<T, C>>(self.entry_point() as *const ()) };
        let (syscall, heap, storage_arg) = callback(use_old_id);
        main(syscall, id, heap, storage_arg)
    }

    fn load_program(&mut self, elf: &ElfFile) -> Result<()> {
        elf.program_iter()
            .filter(|ph| ph.get_type() == Ok(Type::Load))
            .for_each(|ph| {
                let start_vaddr = ph.virtual_addr() as usize + self.virt_start;
                let end_vaddr = start_vaddr + ph.mem_size() as usize;
                let mut permission = DomainMappingFlags::empty();
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    permission |= DomainMappingFlags::READ;
                }
                if ph_flags.is_write() {
                    permission |= DomainMappingFlags::WRITE;
                }
                if ph_flags.is_execute() {
                    permission |= DomainMappingFlags::EXECUTE;
                }
                let vaddr = VirtAddr::from(start_vaddr).align_down_4k().as_usize();
                let end_vaddr = VirtAddr::from(end_vaddr).align_up_4k().as_usize();
                // log::error!(
                //     "map range: [{:#x}-{:#x}], memsize:{:#x}, perm:{:?}",
                //     vaddr,
                //     end_vaddr,
                //     ph.mem_size(),
                //     permission
                // );
                let data =
                    &elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize];
                let data_len = data.len();
                // direct copy data to kernel space
                let module_area = self.module_area.as_ref().unwrap();
                let module_slice = module_area.as_mut_slice();
                let copy_start = start_vaddr - self.virt_start;
                module_slice[copy_start..copy_start + data_len].copy_from_slice(data);
                // log::error!(
                //     "copy data to {:#x}-{:#x}",
                //     copy_start,
                //     copy_start + data_len
                // );
                if permission.contains(DomainMappingFlags::EXECUTE) {
                    self.text_section = vaddr..end_vaddr;
                }
            });
        Ok(())
    }
    fn relocate_dyn(&self, elf: &ElfFile) -> Result<()> {
        if let Ok(res) = relocate_dyn(elf, self.virt_start) {
            trace!("Relocate_dyn {} entries", res.len());
            res.into_iter().for_each(|kv| {
                trace!("relocate: {:#x} -> {:#x}", kv.0, kv.1);
                let addr = kv.0;
                unsafe { (addr as *mut usize).write(kv.1) }
            });
            trace!("Relocate_dyn done");
        }
        Ok(())
    }

    pub fn load(&mut self) -> Result<()> {
        let data = self.data.clone();
        let elf_binary = data.as_slice();
        const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
        if elf_binary[0..4] != ELF_MAGIC {
            return Err("not a elf file");
        }
        debug!("Domain address:{:p}", elf_binary.as_ptr());
        let elf = ElfFile::new(elf_binary)?;
        debug!("Domain type:{:?}", elf.header.pt2.type_().as_type());
        let end_paddr = elf
            .program_iter()
            .filter(|ph| ph.get_type() == Ok(Type::Load))
            .last()
            .map(|x| x.virtual_addr() as usize + x.mem_size() as usize)
            .unwrap();
        let end_paddr = VirtAddr::from(end_paddr).align_up(FRAME_SIZE);
        // alloc free page to map elf
        let module_area = V::map_domain_area(end_paddr.as_usize());
        let region_start = module_area.start_virtual_address().as_usize();
        // log::error!(
        //     "region range:{:#x}-{:#x}",
        //     region_start,
        //     region_start + end_paddr.as_usize()
        // );
        self.virt_start = region_start;
        self.module_area = Some(module_area);
        self.load_program(&elf)?;
        self.relocate_dyn(&elf)?;
        // update text section permission
        let text_pages = (self.text_section.end - self.text_section.start) / FRAME_SIZE;
        V::set_memory_x(self.text_section.start, text_pages)?;
        // log::error!(
        //     "set_memory_x range: {:#x}-{:#x}",
        //     self.text_section.start,
        //     self.text_section.end
        // );
        let entry = elf.header.pt2.entry_point() as usize + region_start;
        // log::error!("entry: {:#x}", entry);
        self.entry_point = entry;
        Ok(())
    }
}

impl<V: DomainVmOps> Drop for DomainLoader<V> {
    fn drop(&mut self) {
        info!("drop domain loader [{}]", self.ident);
        if let Some(module_area) = self.module_area.take() {
            V::unmap_domain_area(module_area)
        }
    }
}

fn relocate_dyn(elf: &ElfFile, region_start: usize) -> Result<Vec<(usize, usize)>> {
    let data = elf
        .find_section_by_name(".rela.dyn")
        .map(|h| h.get_data(elf).unwrap())
        .ok_or("corrupted .rela.dyn")?;
    let entries = match data {
        SectionData::Rela64(entries) => entries,
        _ => return Err("bad .rela.dyn"),
    };
    let mut res = vec![];
    for entry in entries.iter() {
        match entry.get_type() {
            RELATIVE => {
                let value = region_start + entry.get_addend() as usize;
                let addr = region_start + entry.get_offset() as usize;
                res.push((addr, value))
            }
            t => unimplemented!("unknown type: {}", t),
        }
    }
    Ok(res)
}

#[cfg(target_arch = "riscv64")]
const R_RISCV_RELATIVE: u32 = 3;
#[cfg(target_arch = "x86_64")]
const R_X86_64_RELATIVE: u32 = 8;

#[cfg(target_arch = "riscv64")]
const RELATIVE: u32 = R_RISCV_RELATIVE;

#[cfg(target_arch = "x86_64")]
const RELATIVE: u32 = R_X86_64_RELATIVE;
