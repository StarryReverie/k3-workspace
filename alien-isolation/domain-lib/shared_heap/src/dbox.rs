//! DBox is a reference counted reference type that is used to share data between domains.
//!
//! Reference: https://std-dev-guide.rust-lang.org/policy/specialization.html
use alloc::collections::BTreeMap;
use core::{
    alloc::Layout,
    any::TypeId,
    fmt::{Debug, Formatter},
    ops::{Deref, DerefMut},
};

use spin::Mutex;

use super::{CustomDrop, RRefable, SharedData, TypeIdentifiable};

#[repr(C)]
pub struct DBox<T>
where
    T: 'static + RRefable,
{
    pub(crate) domain_id_pointer: *mut u64,
    pub(crate) value_pointer: *mut T,
    pub(crate) exist: bool,
}

unsafe impl<T: RRefable> RRefable for DBox<T> {}
unsafe impl<T: RRefable> Send for DBox<T> where T: Send {}
unsafe impl<T: RRefable> Sync for DBox<T> where T: Sync {}

pub fn drop_no_type<T: CustomDrop>(ptr: *mut u8) {
    let ptr = ptr as *mut T;
    unsafe { &mut *ptr }.custom_drop();
}

type DropFn = fn(ptr: *mut u8);
static DROP: Mutex<BTreeMap<TypeId, DropFn>> = Mutex::new(BTreeMap::new());

pub fn drop_domain_share_data(id: TypeId, ptr: *mut u8) {
    let drop = DROP.lock();
    let drop_fn = drop.get(&id).unwrap();
    drop_fn(ptr);
}

impl<T: RRefable> DBox<T>
where
    T: TypeIdentifiable,
{
    pub(crate) unsafe fn new_with_layout(value: T, layout: Layout, init: bool) -> DBox<T> {
        let type_id = T::type_id();
        let mut drop_guard = DROP.lock();
        drop_guard.entry(type_id).or_insert(drop_no_type::<T>);
        drop(drop_guard);

        let allocation = match crate::share_heap_alloc(layout, type_id, drop_domain_share_data) {
            Some(allocation) => allocation,
            None => panic!("Shared heap allocation failed"),
        };
        let value_pointer = allocation.value_pointer as *mut T;
        *allocation.domain_id_pointer = crate::domain_id();
        if init {
            core::ptr::write(value_pointer, value);
        }
        DBox {
            domain_id_pointer: allocation.domain_id_pointer,
            value_pointer,
            exist: false,
        }
    }

    pub fn new(value: T) -> DBox<T> {
        let layout = Layout::new::<T>();
        unsafe { Self::new_with_layout(value, layout, true) }
    }

    pub fn new_aligned(value: T, align: usize) -> DBox<T> {
        let size = core::mem::size_of::<T>();
        let layout = unsafe { Layout::from_size_align_unchecked(size, align) };
        unsafe { Self::new_with_layout(value, layout, true) }
    }

    pub fn new_uninit() -> DBox<T> {
        let layout = Layout::new::<T>();
        unsafe {
            Self::new_with_layout(
                core::mem::MaybeUninit::uninit().assume_init(),
                layout,
                false,
            )
        }
    }

    pub fn new_uninit_aligned(align: usize) -> DBox<T> {
        let size = core::mem::size_of::<T>();
        let layout = unsafe { Layout::from_size_align_unchecked(size, align) };
        unsafe {
            Self::new_with_layout(
                core::mem::MaybeUninit::uninit().assume_init(),
                layout,
                false,
            )
        }
    }

    pub fn domain_id(&self) -> u64 {
        unsafe { *self.domain_id_pointer }
    }
}

impl<T: RRefable> Deref for DBox<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.value_pointer }
    }
}

impl<T: RRefable> DerefMut for DBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value_pointer }
    }
}

impl<T: RRefable> Drop for DBox<T> {
    fn drop(&mut self) {
        if self.exist {
            return;
        }
        log::debug!("<drop> for DBox {:#x}", self.value_pointer as usize);
        self.custom_drop();
    }
}

impl<T: RRefable> CustomDrop for DBox<T> {
    fn custom_drop(&mut self) {
        if self.exist {
            return;
        }
        log::debug!("<custom_drop> for DBox {:#x}", self.value_pointer as usize);
        let value = unsafe { &mut *self.value_pointer };
        value.custom_drop();
        crate::share_heap_dealloc(self.value_pointer as *mut u8);
    }
}

impl<T: RRefable + Debug> Debug for DBox<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let value = unsafe { &*self.value_pointer };
        let domain_id = unsafe { *self.domain_id_pointer };
        f.debug_struct("DBox")
            .field("value", value)
            .field("domain_id", &domain_id)
            .finish()
    }
}

impl<T: RRefable> SharedData for DBox<T> {
    fn move_to(&self, new_domain_id: u64) -> u64 {
        unsafe {
            let old_domain_id = *self.domain_id_pointer;
            *self.domain_id_pointer = new_domain_id;
            old_domain_id
        }
    }
}
