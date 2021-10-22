#![allow(dead_code)]
use crate::sync::{AtomicPtr, Ordering};
use std::ops::Deref;

pub mod domain;
mod hazard_ptr;
mod sync;

use crate::domain::Domain;
use hazard_ptr::HazPtr;

const SHARED_DOMAIN_ID: usize = 0;

#[cfg(not(loom))]
static SHARED_DOMAIN: Domain<SHARED_DOMAIN_ID> = Domain::default();

#[derive(Debug)]
pub struct AtomBox<'domain, T, const DOMAIN_ID: usize> {
    ptr: AtomicPtr<T>,
    domain: &'domain Domain<DOMAIN_ID>,
}

#[cfg(not(loom))]
impl<T> AtomBox<'static, T, SHARED_DOMAIN_ID> {
    pub fn new(value: T) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self {
            ptr,
            domain: &SHARED_DOMAIN,
        }
    }

    pub fn new_static(value: T) -> &'static mut Self {
        Box::leak(Box::new(Self::new(value)))
    }
}

impl<'domain, T, const DOMAIN_ID: usize> AtomBox<'domain, T, DOMAIN_ID> {
    pub fn new_with_domain(value: T, domain: &'domain Domain<DOMAIN_ID>) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self { ptr, domain }
    }

    pub fn load(&self) -> HazardLoadGuard<'domain, T, DOMAIN_ID> {
        let haz_ptr = self.domain.acquire_haz_ptr();
        // load pointer
        let mut original_ptr = self.ptr.load(Ordering::Relaxed);

        let ptr = loop {
            // protect pointer
            haz_ptr.protect(original_ptr as *mut usize);

            std::sync::atomic::fence(Ordering::SeqCst);

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

    pub fn store(&self, value: T) {
        let _ = self.swap(value);
    }

    pub fn swap(&self, new_value: T) -> HazardStoreGuard<'domain, T, DOMAIN_ID> {
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
        new_value: HazardStoreGuard<'domain, T, DOMAIN_ID>,
    ) -> HazardStoreGuard<'domain, T, DOMAIN_ID> {
        assert!(
            std::ptr::eq(new_value.domain, self.domain),
            "Cannot use guarded value from different domain"
        );

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
        current_value: HazardLoadGuard<'domain, T, DOMAIN_ID>,
        new_value: T,
    ) -> Result<HazardStoreGuard<'domain, T, DOMAIN_ID>, HazardLoadGuard<'domain, T, DOMAIN_ID>>
    {
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
        current_value: HazardLoadGuard<'domain, T, DOMAIN_ID>,
        new_value: HazardStoreGuard<'domain, T, DOMAIN_ID>,
    ) -> Result<
        HazardStoreGuard<'domain, T, DOMAIN_ID>,
        (
            HazardLoadGuard<'domain, T, DOMAIN_ID>,
            HazardStoreGuard<'domain, T, DOMAIN_ID>,
        ),
    > {
        assert!(
            std::ptr::eq(new_value.domain, self.domain),
            "Cannot use guarded value from different domain"
        );

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

    pub fn compare_exchange_weak(
        &self,
        current_value: HazardLoadGuard<'domain, T, DOMAIN_ID>,
        new_value: T,
    ) -> Result<HazardStoreGuard<'domain, T, DOMAIN_ID>, HazardLoadGuard<'domain, T, DOMAIN_ID>>
    {
        let new_ptr = Box::into_raw(Box::new(new_value));
        match self.ptr.compare_exchange_weak(
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
    pub fn compare_exchange_weak_with_guard(
        &self,
        current_value: HazardLoadGuard<'domain, T, DOMAIN_ID>,
        new_value: HazardStoreGuard<'domain, T, DOMAIN_ID>,
    ) -> Result<
        HazardStoreGuard<'domain, T, DOMAIN_ID>,
        (
            HazardLoadGuard<'domain, T, DOMAIN_ID>,
            HazardStoreGuard<'domain, T, DOMAIN_ID>,
        ),
    > {
        assert!(
            std::ptr::eq(new_value.domain, self.domain),
            "Cannot use guarded value from different domain"
        );

        let new_ptr = new_value.ptr;
        match self.ptr.compare_exchange_weak(
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
}

pub struct HazardStoreGuard<'domain, T, const DOMAIN_ID: usize> {
    ptr: *const T,
    domain: &'domain Domain<DOMAIN_ID>,
}

impl<T, const DOMAIN_ID: usize> Deref for HazardStoreGuard<'_, T, DOMAIN_ID> {
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

impl<T, const DOMAIN_ID: usize> Drop for HazardStoreGuard<'_, T, DOMAIN_ID> {
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

pub struct HazardLoadGuard<'domain, T, const DOMAIN_ID: usize> {
    ptr: *const T,
    domain: &'domain Domain<DOMAIN_ID>,
    haz_ptr: Option<&'domain HazPtr>,
}

impl<T, const DOMAIN_ID: usize> Drop for HazardLoadGuard<'_, T, DOMAIN_ID> {
    fn drop(&mut self) {
        if let Some(haz_ptr) = self.haz_ptr {
            haz_ptr.reset();
            haz_ptr.release();
        }
    }
}

impl<T, const DOMAIN_ID: usize> Deref for HazardLoadGuard<'_, T, DOMAIN_ID> {
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

#[cfg(not(loom))]
#[cfg(test)]
mod test {
    use super::*;

    pub(crate) use std::sync::atomic::AtomicUsize;

    static TEST_DOMAIN: domain::Domain<1> = Domain::new(domain::ReclaimStrategy::Eager);

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
        let atom_box: &'static _ = AtomBox::new_static(50);

        let value = atom_box.load();
        assert_eq!(*value, 50);
        let handle1 = std::thread::spawn(move || {
            let h_box = atom_box;
            let value = h_box.load();
            assert_eq!(*value, 50);
        });
        let handle2 = std::thread::spawn(move || {
            let h_box = atom_box;
            let value = h_box.load();
            assert_eq!(*value, 50);
        });
        handle1.join().unwrap();
        handle2.join().unwrap();
    }

    #[test]
    fn single_thread_retire() {
        let atom_box = AtomBox::new(20);

        let value = atom_box.load();
        assert_eq!(*value, 20);
        assert_eq!(
            value.ptr,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire)
        );

        {
            // Immediately retire the original value
            let guard = atom_box.swap(30);
            assert_eq!(guard.ptr, value.ptr);
            let new_value = atom_box.load();
            assert_eq!(*new_value, 30);
        }
        assert_eq!(*value, 20);
        drop(value);
        let _ = atom_box.swap(40);
        let final_value = atom_box.load();
        assert_eq!(*final_value, 40);
    }

    #[test]
    fn drop_test() {
        let drop_count = AtomicUsize::new(0);
        let value = DropTester {
            drop_count: &drop_count,
            value: 20,
        };
        let atom_box = AtomBox::new_with_domain(value, &TEST_DOMAIN);

        let value = atom_box.load();
        assert_eq!(drop_count.load(Ordering::Acquire), 0);
        assert_eq!(**value, 20);
        assert_eq!(
            value.ptr as *mut usize,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire)
        );

        {
            // Immediately retire the original value
            let guard = atom_box.swap(DropTester {
                drop_count: &drop_count,
                value: 30,
            });
            assert_eq!(guard.ptr, value.ptr);
            let new_value = atom_box.load();
            assert_eq!(**new_value, 30);
        }
        assert_eq!(
            drop_count.load(Ordering::Acquire),
            0,
            "Value should not be dropped while there is an active reference to it"
        );
        assert_eq!(**value, 20);
        drop(value);
        let _ = atom_box.swap(DropTester {
            drop_count: &drop_count,
            value: 40,
        });
        let final_value = atom_box.load();
        assert_eq!(**final_value, 40);
        assert_eq!(drop_count.load(Ordering::SeqCst), 2);
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
        let atom_box1 = AtomBox::new_with_domain(value1, &TEST_DOMAIN);
        let atom_box2 = AtomBox::new_with_domain(value2, &TEST_DOMAIN);

        {
            // Immediately retire the original value
            let guard1 = atom_box1.swap(DropTester {
                drop_count: &drop_count_for_placeholder,
                value: 30,
            });
            let guard2 = atom_box2.swap_with_guarded_value(guard1);
            let _ = atom_box1.swap_with_guarded_value(guard2);
            let new_value1 = atom_box1.load();
            let new_value2 = atom_box2.load();
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
