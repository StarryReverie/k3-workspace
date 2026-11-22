#![feature(allocator_api)]
#![feature(downcast_unchecked)]
#![no_std]
#![no_main]
extern crate alloc;
use alloc::{boxed::Box, sync::Arc};
use core::{
    alloc::{AllocError, Allocator, Layout},
    any::Any,
    ptr::NonNull,
};

use spin::Once;
pub trait SendAllocator: Allocator + Send + Sync {}
type ArcValueType = Arc<dyn Any + Send + Sync, CustomStorge>;

pub trait DomainDataStorage: Send + Sync {
    fn insert(&self, key: &str, value: ArcValueType) -> Option<ArcValueType>;
    fn get(&self, key: &str) -> Option<ArcValueType>;
    fn remove(&self, key: &str) -> Option<ArcValueType>;
}

/// A custom allocator which allocates memory from the custom heap
///
/// This allocator is used to allocate memory for the domain's state data.
#[derive(Debug, Clone)]
pub struct CustomStorge;

unsafe impl Allocator for CustomStorge {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        log::debug!(
            "[CustomStorge][{}] allocate memory with layout: {:?}",
            shared_heap::domain_id(),
            layout
        );
        let alloc = *DATA_ALLOCATOR.get().unwrap();
        alloc.allocate(layout)
    }
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        log::debug!(
            "[CustomStorge][{}] deallocate memory with layout: {:?}",
            shared_heap::domain_id(),
            layout
        );
        let alloc = *DATA_ALLOCATOR.get().unwrap();
        alloc.deallocate(ptr, layout)
    }
}

type DataStorageHeap = &'static dyn SendAllocator;
static DATA_ALLOCATOR: Once<DataStorageHeap> = Once::new();

pub fn init_data_allocator(allocator: &'static dyn SendAllocator) {
    DATA_ALLOCATOR.call_once(|| allocator);
    log::info!("init data allocator success");
}

pub struct StorageArg {
    pub allocator: DataStorageHeap,
    pub storage: Box<dyn DomainDataStorage>,
}

impl StorageArg {
    pub fn new(allocator: DataStorageHeap, storage: Box<dyn DomainDataStorage>) -> Self {
        Self { allocator, storage }
    }
}

#[cfg(feature = "impl")]
mod __private {
    use alloc::{boxed::Box, sync::Arc};
    use core::any::Any;

    use spin::Once;

    use crate::{ArcValueType, CustomStorge, DataStorageHeap, DomainDataStorage};

    pub fn insert<T: Any + Send + Sync>(key: &str, value: T) -> Option<Arc<T, CustomStorge>> {
        let arc = Arc::new_in(value, CustomStorge);
        let old = DATABASE.get().unwrap().insert(key, arc);
        let old_arc = match old {
            Some(arc) => unsafe {
                let v = arc.downcast_unchecked::<T>();
                Some(v)
            },
            None => None,
        };
        old_arc
    }

    pub fn get<T: Any + Send + Sync>(key: &str) -> Option<Arc<T, CustomStorge>> {
        let res = DATABASE
            .get()
            .unwrap()
            .get(key)
            .and_then(|arc_wrapper| unsafe {
                let res = arc_wrapper.downcast_unchecked();
                Some(res)
            });
        res
    }

    pub fn get_or_insert<T: Any + Send + Sync, F: FnOnce() -> T>(
        key: &str,
        f: F,
    ) -> Arc<T, CustomStorge> {
        let arc = get::<T>(&key);
        match arc {
            Some(arc) => arc,
            None => {
                let value = f();
                let arc = Arc::new_in(value, CustomStorge);
                DATABASE.get().unwrap().insert(key, arc.clone());
                arc
            }
        }
    }

    pub fn get_or_insert_in<T: Any + Send + Sync, F: FnOnce() -> Arc<T, CustomStorge>>(
        key: &str,
        f: F,
    ) -> Arc<T, CustomStorge> {
        let arc = get::<T>(&key);
        match arc {
            Some(arc) => arc,
            None => {
                let value = f();
                DATABASE.get().unwrap().insert(key, value.clone());
                value
            }
        }
    }

    pub fn remove<T: Any + Send + Sync>(key: &str) -> Option<Arc<T, CustomStorge>> {
        let res = DATABASE
            .get()
            .unwrap()
            .remove(key)
            .and_then(|value| unsafe {
                let strong_count = Arc::strong_count(&value);
                log::info!("remove_data: {:?}, ref count: {}", key, strong_count);
                assert!(strong_count >= 2);
                let value = value.downcast_unchecked::<T>();
                let value = if strong_count != 2 {
                    // decrement strong count to 2
                    let raw = Arc::into_raw(value);
                    for _ in 0..(strong_count - 2) {
                        Arc::decrement_strong_count_in(raw, CustomStorge);
                    }
                    Arc::from_raw_in(raw, CustomStorge)
                } else {
                    value
                };
                let strong_count = Arc::strong_count(&value);
                log::info!("remove_data: {:?}, ref count: {}", key, strong_count);
                Some(value)
            });
        res
    }

    static DATABASE: Once<Box<dyn DomainDataStorage>> = Once::new();

    pub fn init_database(database: Box<dyn DomainDataStorage>) {
        DATABASE.call_once(|| database);
        log::info!("init database success");
    }
}

#[cfg(feature = "impl")]
pub use __private::*;
