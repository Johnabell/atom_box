use crate::sync::{AtomicBool, AtomicPtr, Ordering};

#[derive(Debug)]
pub struct HazPtr {
    pub(crate) ptr: AtomicPtr<usize>,
    pub(crate) active: AtomicBool,
}

impl HazPtr {
    pub(crate) fn new(active: bool) -> Self {
        Self {
            ptr: AtomicPtr::new(core::ptr::null_mut()),
            active: AtomicBool::new(active),
        }
    }

    pub(crate) fn reset(&self) {
        self.ptr.store(core::ptr::null_mut(), Ordering::Release);
    }

    pub(crate) fn protect(&self, ptr: *mut usize) {
        self.ptr.store(ptr, Ordering::Release);
    }

    pub(crate) fn release(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub(crate) fn try_acquire(&self) -> bool {
        let active = self.active.load(Ordering::Acquire);
        !active
            && self
                .active
                .compare_exchange(active, true, Ordering::Release, Ordering::Relaxed)
                .is_ok()
    }
}
