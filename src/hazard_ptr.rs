use crate::sync::{AtomicPtr, Ordering};

#[derive(Debug)]
pub struct HazPtr {
    pub(crate) ptr: AtomicPtr<usize>,
}

impl HazPtr {
    pub(crate) fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    pub(crate) fn reset(&self) {
        self.ptr.store(core::ptr::null_mut(), Ordering::Release);
    }

    pub(crate) fn protect(&self, ptr: *mut usize) {
        self.ptr.store(ptr, Ordering::Release);
    }
}
