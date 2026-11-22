use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
};

#[inline(always)]
pub fn hart_id() -> usize {
    arch::hart_id()
}

pub struct CpuLocal<T>(UnsafeCell<T>);

unsafe impl<T> Sync for CpuLocal<T> {}

impl<T> CpuLocal<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[allow(clippy::mut_from_ref)]
    pub fn as_mut(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

impl<T> Deref for CpuLocal<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.0.get() }
    }
}

impl<T> DerefMut for CpuLocal<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}
