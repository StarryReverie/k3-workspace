use alloc::boxed::Box;
use core::{
    alloc::Layout,
    fmt::{Debug, Formatter},
    mem::MaybeUninit,
    ops::{Deref, DerefMut, Index, IndexMut},
};

use super::{CustomDrop, DBox, RRefable, SharedData, TypeIdentifiable};

pub struct DVec<T>
where
    T: 'static + RRefable + Copy + TypeIdentifiable,
{
    data: DBox<T>,
    size: usize,
    exist: bool,
}
unsafe impl<T> RRefable for DVec<T> where T: 'static + RRefable + Copy + TypeIdentifiable {}
unsafe impl<T> Send for DVec<T> where T: 'static + RRefable + Copy + TypeIdentifiable {}

impl<T> DVec<T>
where
    T: 'static + RRefable + Copy + TypeIdentifiable,
{
    pub fn new(initial_value: T, size: usize) -> Self {
        let layout = Layout::array::<T>(size).unwrap();
        let data = unsafe { DBox::new_with_layout(initial_value, layout, false) };
        let mut vec = Self {
            data,
            size,
            exist: false,
        };
        vec.as_mut_slice().fill(initial_value);
        vec
    }

    pub fn new_uninit(size: usize) -> Self {
        let layout = Layout::array::<T>(size).unwrap();
        let data =
            unsafe { DBox::new_with_layout(MaybeUninit::uninit().assume_init(), layout, false) };
        Self {
            data,
            size,
            exist: false,
        }
    }

    #[allow(clippy::uninit_assumed_init)]
    pub fn from_slice(slice: &[T]) -> Self {
        let size = slice.len();
        let layout = Layout::array::<T>(size).unwrap();
        let data =
            unsafe { DBox::new_with_layout(MaybeUninit::uninit().assume_init(), layout, false) };
        let mut vec = Self {
            data,
            size,
            exist: false,
        };
        vec.as_mut_slice().copy_from_slice(slice);
        vec
    }
    pub fn as_slice(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(&*self.data, self.size) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { core::slice::from_raw_parts_mut(&mut *self.data, self.size) }
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// # WARNING
    /// This is a super dangerous function, it will return a slice of the data without checking the domain id
    pub fn from_other_rvec_slice(slice: &[T]) -> Self {
        let id = Box::new(crate::domain_id());
        let ptr = Box::into_raw(id);
        let shared_heap = DBox {
            domain_id_pointer: ptr,
            value_pointer: slice.as_ptr() as *mut T,
            exist: true,
        };
        Self {
            data: shared_heap,
            size: slice.len(),
            exist: true,
        }
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> Index<usize> for DVec<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> IndexMut<usize> for DVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.as_mut_slice()[index]
    }
}

impl<T> Debug for DVec<T>
where
    T: 'static + RRefable + Copy + TypeIdentifiable + Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DVec")
            .field("data", &self.data)
            .field("size", &self.size)
            .finish()
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> Drop for DVec<T> {
    fn drop(&mut self) {
        unsafe {
            if self.exist {
                let id = self.data.domain_id_pointer;
                let _d = Box::from_raw(id);
                return;
            }
        }
        log::debug!("<drop> for DVec");
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> CustomDrop for DVec<T> {
    fn custom_drop(&mut self) {
        if self.exist {
            return;
        }
        log::debug!("<custom_drop> for DVec");
        self.data.custom_drop();
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> SharedData for DVec<T> {
    fn move_to(&self, new_domain_id: u64) -> u64 {
        self.data.move_to(new_domain_id)
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> Deref for DVec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: RRefable + Copy + TypeIdentifiable> DerefMut for DVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}
