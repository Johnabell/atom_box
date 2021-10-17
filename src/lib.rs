#![allow(dead_code)]
#![feature(const_fn_trait_bound)]
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

mod domain;
use crate::domain::Domain;

static SHARED_DOMAIN: Domain = Domain::default();

#[derive(Debug)]
pub struct HazPtr {
    pub(crate) ptr: AtomicPtr<usize>,
    pub(crate) active: AtomicBool,
}

impl HazPtr {
    pub(crate) fn new(active: bool) -> Self {
        Self {
            ptr: AtomicPtr::new(std::ptr::null_mut()),
            active: AtomicBool::new(active),
        }
    }

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
struct HazardBox<'domain, T> {
    ptr: AtomicPtr<T>,
    domain: &'domain Domain,
}

impl<'domain, T> HazardBox<'domain, T> {
    pub fn new(value: T) -> Self {
        Self::new_with_domain(value, &SHARED_DOMAIN)
    }

    pub fn new_static(value: T) -> &'static mut Self {
        Box::leak(Box::new(HazardBox::new(value)))
    }

    pub fn new_with_domain(value: T, domain: &'domain Domain) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self { ptr, domain }
    }

    pub fn load(&self) -> HazardLoadGuard<'domain, T> {
        let haz_ptr = self.domain.acquire_haz_ptr();
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

    /// Store a new value in the guarded pointer.
    ///
    /// # Panic
    ///
    /// Function panics if a guarded object from another domain is passed to this function.
    pub fn swap_with_guarded_value(
        &self,
        new_value: HazardStoreGuard<'domain, T>,
    ) -> HazardStoreGuard<'domain, T> {
        // TODO: can we make this statically enforced?
        // if new_value.domain != self.domain {
        //     panic!("Cannot use guarded value from different domain");
        // }
        let new_ptr = new_value.ptr;
        std::mem::forget(new_value);
        let old_ptr = self.ptr.swap(new_ptr as *mut T, Ordering::AcqRel);
        HazardStoreGuard {
            ptr: old_ptr,
            domain: self.domain,
        }
    }

    pub fn compare_exchange(
        &self,
        current_value: HazardLoadGuard<'domain, T>,
        new_value: T,
    ) -> Result<HazardStoreGuard<'domain, T>, HazardLoadGuard<'domain, T>> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        match self.ptr.compare_exchange(
            current_value.ptr as *mut T,
            new_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(ptr) => Ok(HazardStoreGuard {
                ptr,
                domain: self.domain,
            }),
            Err(ptr) => Err(HazardLoadGuard {
                ptr,
                domain: self.domain,
                haz_ptr: None,
            }),
        }
    }

    /// Store a new value in the guarded pointer if its value matchs `current_value`.
    ///
    /// # Panic
    ///
    /// Function panics if a guarded object from another domain is passed to this function.
    pub fn compare_exchange_with_guard(
        &self,
        current_value: HazardLoadGuard<'domain, T>,
        new_value: HazardStoreGuard<'domain, T>,
    ) -> Result<
        HazardStoreGuard<'domain, T>,
        (HazardLoadGuard<'domain, T>, HazardStoreGuard<'domain, T>),
    > {
        // TODO: can we make this statically enforced?
        // if new_value.domain != self.domain {
        //     panic!("Cannot use guarded value from different domain");
        // }
        let new_ptr = new_value.ptr;
        match self.ptr.compare_exchange(
            current_value.ptr as *mut T,
            new_ptr as *mut T,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(ptr) => {
                std::mem::forget(new_value);
                Ok(HazardStoreGuard {
                    ptr,
                    domain: self.domain,
                })
            }
            Err(ptr) => Err((
                HazardLoadGuard {
                    ptr,
                    domain: self.domain,
                    haz_ptr: None,
                },
                new_value,
            )),
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
        // # Safety
        //
        // The pointer to this object was originally created via box into raw.
        // The heap alocated value cannot be dropped via external code.
        // We are the only person with this pointer in a store guard. There might
        // be other people referencing it as a read only value where it is protected
        // via hazard pointers.
        // We are safe to flag it for retire, where it will be reclaimed when it is no longer
        // protected by any hazard pointers.
        unsafe { self.domain.retire(self.ptr as *mut T) };
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
    use std::sync::atomic::AtomicUsize;

    static TEST_DOMAIN: domain::Domain = domain::Domain::new(domain::ReclaimStrategy::Eager);

    struct DropTester<'a, T> {
        drop_count: &'a AtomicUsize,
        value: T,
    }

    impl<'a, T> Drop for DropTester<'a, T> {
        fn drop(&mut self) {
            self.drop_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    impl<'a, T> Deref for DropTester<'a, T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            &self.value
        }
    }

    #[test]
    fn api_test() {
        let hazard_box: &'static _ = HazardBox::new_static(50);

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

    #[test]
    fn single_thread_retire() {
        let hazard_box = HazardBox::new(20);

        let value = hazard_box.load();
        assert_eq!(*value, 20);
        assert_eq!(
            value.ptr,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire)
        );

        {
            // Immediately retire the original value
            let guard = hazard_box.swap(30);
            assert_eq!(guard.ptr, value.ptr);
            let new_value = hazard_box.load();
            assert_eq!(*new_value, 30);
        }
        assert_eq!(*value, 20);
        drop(value);
        let _ = hazard_box.swap(40);
        let final_value = hazard_box.load();
        assert_eq!(*final_value, 40);
    }

    #[test]
    fn drop_test() {
        let drop_count = AtomicUsize::new(0);
        let value = DropTester {
            drop_count: &drop_count,
            value: 20,
        };
        let hazard_box = HazardBox::new_with_domain(value, &TEST_DOMAIN);

        let value = hazard_box.load();
        assert_eq!(drop_count.load(Ordering::Acquire), 0);
        assert_eq!(**value, 20);
        assert_eq!(
            value.ptr as *mut usize,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire)
        );

        {
            // Immediately retire the original value
            let guard = hazard_box.swap(DropTester {
                drop_count: &drop_count,
                value: 30,
            });
            assert_eq!(guard.ptr, value.ptr);
            let new_value = hazard_box.load();
            assert_eq!(**new_value, 30);
        }
        assert_eq!(
            drop_count.load(Ordering::Acquire),
            0,
            "Value should not be dropped while there is an active reference to it"
        );
        assert_eq!(**value, 20);
        drop(value);
        let _ = hazard_box.swap(DropTester {
            drop_count: &drop_count,
            value: 40,
        });
        let final_value = hazard_box.load();
        assert_eq!(**final_value, 40);
        assert_eq!(drop_count.load(Ordering::Acquire), 2);
    }

    #[test]
    fn swap_with_gaurd_test() {
        let drop_count = AtomicUsize::new(0);
        let drop_count_for_placeholder = AtomicUsize::new(0);
        let value1 = DropTester {
            drop_count: &drop_count,
            value: 10,
        };
        let value2 = DropTester {
            drop_count: &drop_count,
            value: 20,
        };
        let hazard_box1 = HazardBox::new_with_domain(value1, &TEST_DOMAIN);
        let hazard_box2 = HazardBox::new_with_domain(value2, &TEST_DOMAIN);

        {
            // Immediately retire the original value
            let guard1 = hazard_box1.swap(DropTester {
                drop_count: &drop_count_for_placeholder,
                value: 30,
            });
            let guard2 = hazard_box2.swap_with_guarded_value(guard1);
            let _ = hazard_box1.swap_with_guarded_value(guard2);
            let new_value1 = hazard_box1.load();
            let new_value2 = hazard_box2.load();
            assert_eq!(**new_value1, 20);
            assert_eq!(**new_value2, 10);
        }
        assert_eq!(
            drop_count_for_placeholder.load(Ordering::Acquire),
            1,
            "The placeholder value should have been dropped"
        );
        assert_eq!(
            drop_count.load(Ordering::Acquire),
            0,
            "Neither of the intial values should have been dropped"
        );
    }
}
