#![allow(dead_code)]
use std::sync::atomic::{Ordering, AtomicPtr, AtomicBool, AtomicUsize};
use std::ops::Deref;

static SHARED_DOMAIN: Domain = Domain::new();

#[derive(Debug)]
struct Domain {
    values: AtomicPtr<RetiredNode>,
    hazard_ptrs: HazardPtrs,
}

#[derive(Debug)]
struct HazardPtrs {
    head: AtomicPtr<HazPtr>,
    count: AtomicUsize,
}

impl Domain {
    const fn new() -> Self {
        Self {
            hazard_ptrs: HazardPtrs {
                head: AtomicPtr::new(std::ptr::null_mut()),
                count: AtomicUsize::new(0),
            },
            values: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    fn aquire_haz_ptr(&self) -> &HazPtr {
        todo!()
    }
    
    fn retire(&self, value: &dyn Retirable) {
        todo!()
    }
}

#[derive(Debug)]
pub struct HazPtr {
    pub(crate) ptr: AtomicPtr<usize>,
    pub(crate) next: AtomicPtr<HazPtr>,
    pub(crate) active: AtomicBool,
}

impl HazPtr {
    pub(crate) fn reset(&self) {
        self.ptr.store(std::ptr::null_mut(), Ordering::Release);
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

#[derive(Debug)]
struct RetiredNode {
    value: *mut dyn Retirable,
    next: AtomicPtr<RetiredNode>
}

trait Retirable {}

impl<T> Retirable for T {}

#[derive(Debug)]
struct HazardBox<'domain, T> {
    ptr: AtomicPtr<T>,
    domain: &'domain Domain,
}

impl<'domain, T> HazardBox<'domain, T> {
    pub fn new(value: T) -> Self {
        Self::new_with_domain(value, &SHARED_DOMAIN)
    }

    pub fn new_with_domain(value: T, domain: &'domain Domain) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self {
            ptr,
            domain,
        } 
    }

    pub fn load(&self) -> HazardLoadGuard<'domain, T> {
        let haz_ptr = self.domain.aquire_haz_ptr();
        // load pointer
        let mut original_ptr = self.ptr.load(Ordering::Relaxed);

        let ptr = loop {
            // protect pointer
            haz_ptr.protect(original_ptr as *mut usize);

            // check pointer
            let current_ptr = self.ptr.load(Ordering::Acquire); 
            if current_ptr == original_ptr {
                // The pointer is the same, we have successfully protected its value.
                break current_ptr;
            }
            haz_ptr.reset();
            original_ptr = current_ptr;
        }; 
        HazardLoadGuard {
            ptr,
            domain: self.domain,
            haz_ptr: Some(haz_ptr),
        }
    }

    pub fn swap(&self, new_value: T) -> HazardStoreGuard<'domain, T> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        HazardStoreGuard {
            ptr: old_ptr,
            domain: self.domain,
        }
    }

    pub fn swap_with_guarded_value(&self, new_value: HazardStoreGuard<'domain, T>) -> HazardStoreGuard<'domain, T> {
        // TODO: is it alright to place in a value from another domain
        let new_ptr = new_value.ptr;
        std::mem::forget(new_value);
        let old_ptr = self.ptr.swap(new_ptr as *mut T, Ordering::AcqRel);
        HazardStoreGuard {
            ptr: old_ptr,
            domain: self.domain,
        }
    }


    pub fn compare_exchange(&self, current_value: HazardLoadGuard<'domain, T>, new_value: T) -> Result<HazardStoreGuard<'domain, T>, HazardLoadGuard<'domain, T>> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        match self.ptr.compare_exchange(current_value.ptr as *mut T, new_ptr, Ordering::AcqRel, Ordering::AcqRel) {
            Ok(ptr) => Ok(HazardStoreGuard { ptr, domain: self.domain }),
            Err(ptr) => Err(HazardLoadGuard { ptr, domain: self.domain, haz_ptr: None }),
        }
    }

    pub fn compare_exchange_with_guard(&self, current_value: HazardLoadGuard<'domain, T>, new_value: HazardStoreGuard<'domain, T>) -> Result<HazardStoreGuard<'domain, T>, HazardLoadGuard<'domain, T>> {
        // TODO: is it alright to place in a value from another domain
        let new_ptr = new_value.ptr;
        std::mem::forget(new_value);
        match self.ptr.compare_exchange(current_value.ptr as *mut T, new_ptr as *mut T, Ordering::AcqRel, Ordering::AcqRel) {
            Ok(ptr) => Ok(HazardStoreGuard { ptr, domain: self.domain }),
            Err(ptr) => Err(HazardLoadGuard { ptr, domain: self.domain, haz_ptr: None }),
        }
    }

    // TODO: implement compare exchange weak
}

struct HazardStoreGuard<'domain, T> {
    ptr: *const T,
    domain: &'domain Domain,
}

impl<T> Deref for HazardStoreGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // # Saftey
        //
        // The pointer is protected by the hazard pointer so will not have been droped
        // The pointer was created via a Box so is aligned and there are no mutable references
        // since we do not give any out.
        //
        unsafe { self.ptr.as_ref().expect("Non null") }
    }
}

impl<T> Drop for HazardStoreGuard<'_, T> {
    fn drop(&mut self) {
        self.domain.retire(&self.ptr);
    }
}


struct HazardLoadGuard<'domain, T> {
    ptr: *const T,
    domain: &'domain Domain,
    haz_ptr: Option<&'domain HazPtr>,
}

impl<T> Drop for HazardLoadGuard<'_, T> {
    fn drop(&mut self) {
        if let Some(haz_ptr) = self.haz_ptr {
            haz_ptr.reset();
            haz_ptr.release();
        }
    }
}

impl<T> Deref for HazardLoadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // # Saftey
        //
        // The pointer is protected by the hazard pointer so will not have been droped
        // The pointer was created via a Box so is aligned and there are no mutable references
        // since we do not give any out.
        //
        unsafe { self.ptr.as_ref().expect("Non null") }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn api_test() {
        let hazard_box: &'static _ = Box::leak(Box::new(HazardBox::new(50)));

        let value = hazard_box.load();
        assert_eq!(*value, 50);
        let handle1 = std::thread::spawn(move || {
            let h_box = hazard_box;
            let value = h_box.load();
            assert_eq!(*value, 50);
        });
        let handle2 = std::thread::spawn(move || {
            let h_box = hazard_box;
            let value = h_box.load();
            assert_eq!(*value, 50);
        });
        handle1.join().unwrap();
        handle2.join().unwrap();
    }
}
