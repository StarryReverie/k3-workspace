#![feature(auto_traits)]
#![feature(negative_impls)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![no_std]
mod dbox;
mod dvec;

extern crate alloc;
use core::{
    alloc::Layout,
    any::{type_name_of_val, TypeId},
};

pub use dbox::DBox;
pub use dvec::DVec;
use spin::Once;
/// A trait for types that can be shared between domains.
///
/// # Safety
/// This trait is unsafe because it is not safe to share all types between domains.
pub unsafe auto trait RRefable {}

impl<T> !RRefable for *mut T {}
impl<T> !RRefable for *const T {}
impl<T> !RRefable for &T {}
impl<T> !RRefable for &mut T {}
impl<T> !RRefable for [T] {}

pub trait TypeIdentifiable {
    fn type_id() -> TypeId;
}

impl<T: 'static> TypeIdentifiable for T {
    fn type_id() -> TypeId {
        TypeId::of::<T>()
    }
}

pub trait CustomDrop {
    fn custom_drop(&mut self);
}

impl<T: RRefable> CustomDrop for T {
    default fn custom_drop(&mut self) {
        log::debug!("default for {}", type_name_of_val(self));
    }
}
impl<T: RRefable> CustomDrop for Option<T> {
    fn custom_drop(&mut self) {
        if let Some(val) = self {
            val.custom_drop();
        }
    }
}

impl<T: RRefable, const N: usize> CustomDrop for [T; N] {
    fn custom_drop(&mut self) {
        for el in self.iter_mut() {
            el.custom_drop();
        }
    }
}

pub trait SharedData {
    fn move_to(&self, new_domain_id: u64) -> u64;
}

impl<T: RRefable> SharedData for T {
    default fn move_to(&self, _new_domain_id: u64) -> u64 {
        0
    }
}

impl<T: RRefable> SharedData for Option<T> {
    fn move_to(&self, new_domain_id: u64) -> u64 {
        match self {
            Some(val) => val.move_to(new_domain_id),
            None => 0,
        }
    }
}

macro_rules! impl_shared_data {
    ($(($index:tt,$t:ident)),*) => {
        impl <T:RRefable> SharedData for ($($t),*){
            #[allow(unused_assignments)]
            fn move_to(&self, new_domain_id: u64)->u64{
                let mut domain_id;
                $(domain_id = self.$index.move_to(new_domain_id);)*
                domain_id
            }
        }
    }
}

impl_shared_data!((0, T), (1, T));
impl_shared_data!((0, T), (1, T), (2, T));
impl_shared_data!((0, T), (1, T), (2, T), (3, T));
impl_shared_data!((0, T), (1, T), (2, T), (3, T), (4, T));
impl_shared_data!((0, T), (1, T), (2, T), (3, T), (4, T), (5, T));
impl_shared_data!((0, T), (1, T), (2, T), (3, T), (4, T), (5, T), (6, T));

#[derive(Copy, Clone)]
pub struct SharedHeapAllocation {
    pub value_pointer: *mut u8,
    pub domain_id_pointer: *mut u64,
    pub layout: Layout,
    pub type_id: TypeId,
    pub drop_fn: fn(TypeId, *mut u8),
}

impl SharedHeapAllocation {
    pub fn domain_id(&self) -> u64 {
        unsafe { *self.domain_id_pointer }
    }
    pub fn drop_fn(&self) {
        (self.drop_fn)(self.type_id, self.value_pointer);
    }
    pub fn set_domain_id(&self, domain_id: u64) {
        unsafe {
            *self.domain_id_pointer = domain_id;
        }
    }
}

unsafe impl Send for SharedHeapAllocation {}

pub trait SharedHeapAlloc: Send + Sync {
    /// Allocates a new heap allocation with the given layout, type_id, and drop function.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the layout is valid and that the drop function is correct.
    unsafe fn alloc(
        &self,
        layout: Layout,
        type_id: TypeId,
        drop_fn: fn(TypeId, *mut u8),
    ) -> Option<SharedHeapAllocation>;
    /// Deallocates the heap allocation at the given pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the pointer is valid and that the allocation was not already deallocated.
    unsafe fn dealloc(&self, ptr: *mut u8);
}

static SHARED_HEAP: Once<&'static dyn SharedHeapAlloc> = Once::new();

static CRATE_DOMAIN_ID: Once<u64> = Once::new();

pub fn init(allocator: &'static dyn SharedHeapAlloc, domain_id: u64) {
    SHARED_HEAP.call_once(|| allocator);
    CRATE_DOMAIN_ID.call_once(|| domain_id);
}

pub fn share_heap_alloc(
    layout: Layout,
    type_id: TypeId,
    drop_fn: fn(TypeId, *mut u8),
) -> Option<SharedHeapAllocation> {
    unsafe { SHARED_HEAP.get_unchecked().alloc(layout, type_id, drop_fn) }
}

pub(crate) fn share_heap_dealloc(ptr: *mut u8) {
    unsafe { SHARED_HEAP.get_unchecked().dealloc(ptr) }
}

#[inline]
pub fn domain_id() -> u64 {
    unsafe { *CRATE_DOMAIN_ID.get_unchecked() }
}
