pub use ksync::{Mutex, MutexGuard, RwLock};
pub use spin::Once;

impl<T> OnceGet<T> for Once<T> {
    fn get_must(&self) -> &T {
        unsafe { self.get_unchecked() }
    }
}

pub trait OnceGet<T> {
    fn get_must(&self) -> &T;
}
