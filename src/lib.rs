//! # Atom Box
//!
//! This crate provides a safe idiomatic Rust API for an Atomic Box with safe memory
//! reclamation when used in multi-threaded concurrent lock-free data structures.
//!
//! Under the covers it uses Hazard Pointers to ensure memory is only reclaimed when all references
//! are dropped.
//!
//! The main type provided is the `AtomBox`.
//!
//! # Examples
//!
//! ```
//! use atom_box::AtomBox;
//! use std::thread;
//!
//! const ITERATIONS: usize = 100000;
//!
//! let atom_box1: &'static _ = AtomBox::new_static(0);
//! let atom_box2: &'static _ = AtomBox::new_static(0);
//!
//! let handle1 = thread::spawn(move || {
//!     let mut current_value = 0;
//!     for _ in 1..=ITERATIONS {
//!         let new_value = atom_box1.load();
//!         assert!(*new_value >= current_value, "Value should not decrease");
//!         current_value = *new_value;
//!     }
//! });
//!
//! let handle2 = thread::spawn(move || {
//!     for i in 1..=ITERATIONS {
//!         let guard1 = atom_box1.swap(i);
//!         let value1 = *guard1;
//!         let guard2 = atom_box2.swap_from_guard(guard1);
//!         assert!(
//!             *guard2 <= value1,
//!             "Value in first box should be greater than or equal to value in second box"
//!         );
//!     }
//! });
//!
//! handle1.join().unwrap();
//! handle2.join().unwrap();
//! ```

#![warn(missing_docs)]
use crate::sync::{AtomicPtr, Ordering};
use std::ops::Deref;

pub mod domain;
mod hazard_ptr;
mod sync;

use crate::domain::Domain;
use hazard_ptr::HazPtr;

#[cfg(not(loom))]
const SHARED_DOMAIN_ID: usize = 0;

#[cfg(not(loom))]
static SHARED_DOMAIN: Domain<SHARED_DOMAIN_ID> = Domain::default();

mod macros {
    // The loom atomics do not have const constructors. So we cannot use them in const functions.
    // This macro enables us to create a const function in normal compilation and a non const
    // function when compiling for loom.
    macro_rules! conditional_const {
        ($( $doc_comment:expr )?, $visibility:vis, $( $token:tt )*) => {
            $( #[doc = $doc_comment] )?
            #[cfg(not(loom))]
            $visibility const $( $token )*
            #[cfg(loom)]
            $visibility $( $token )*
        };
    }
    pub(crate) use conditional_const;
}

/// A box which can safely be shared between threads and atomically updated.
///
/// Memory will be safely reclaimed after all threads have dropped their references to any give
/// value.
///
/// # Example
///
/// ```
/// use atom_box::AtomBox;
/// use std::thread;
///
/// const ITERATIONS: usize = 100000;
///
/// let atom_box1: &'static _ = AtomBox::new_static(0);
/// let atom_box2: &'static _ = AtomBox::new_static(0);
///
/// let handle1 = thread::spawn(move || {
///     let mut current_value = 0;
///     for _ in 1..=ITERATIONS {
///         let new_value = atom_box1.load();
///         assert!(*new_value >= current_value, "Value should not decrease");
///         current_value = *new_value;
///     }
/// });
///
/// let handle2 = thread::spawn(move || {
///     for i in 1..=ITERATIONS {
///         let guard1 = atom_box1.swap(i);
///         let value1 = *guard1;
///         let guard2 = atom_box2.swap_from_guard(guard1);
///         assert!(
///             *guard2 <= value1,
///             "Value in first box should be greater than or equal to value in second box"
///         );
///     }
/// });
///
/// handle1.join().unwrap();
/// handle2.join().unwrap();
/// ```
#[derive(Debug)]
pub struct AtomBox<'domain, T, const DOMAIN_ID: usize> {
    ptr: AtomicPtr<T>,
    domain: &'domain Domain<DOMAIN_ID>,
}

#[cfg(not(loom))]
impl<T> AtomBox<'static, T, SHARED_DOMAIN_ID> {
    /// Creates a new `AtomBox` associated with the shared (global) domain.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new("Hello");
    ///
    /// let value = atom_box.load();
    /// assert_eq!(*value, "Hello");
    ///
    /// atom_box.store("World");
    /// let value = atom_box.load();
    /// assert_eq!(*value, "World");
    /// ```
    pub fn new(value: T) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self {
            ptr,
            domain: &SHARED_DOMAIN,
        }
    }

    /// Creates a new `AtomBox` with a static lifetime.
    ///
    /// A convenience constructor for `Box::leak(Box::new(Self::new(value)))`.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    /// let atom_box: &'static _ = AtomBox::new_static(50);
    /// let value = atom_box.load();
    ///
    /// assert_eq!(
    ///     *value, 50,
    ///     "We are able to get the original value by loading it"
    /// );
    /// let handle1 = std::thread::spawn(move || {
    ///     let h_box = atom_box;
    ///     let value = h_box.load();
    ///     assert_eq!(
    ///         *value, 50,
    ///         "The value should be accessible in multiple threads"
    ///     );
    /// });
    /// let handle2 = std::thread::spawn(move || {
    ///     let h_box = atom_box;
    ///     let value = h_box.load();
    ///     assert_eq!(
    ///         *value, 50,
    ///         "The value should be accessible in multiple threads"
    ///     );
    /// });
    /// handle1.join().unwrap();
    /// handle2.join().unwrap();
    /// ```
    pub fn new_static(value: T) -> &'static mut Self {
        Box::leak(Box::new(Self::new(value)))
    }
}

impl<'domain, T, const DOMAIN_ID: usize> AtomBox<'domain, T, DOMAIN_ID> {
    /// Creates a new `AtomBox` and assoicates it with the given domain.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::{AtomBox, domain::Domain, domain::ReclaimStrategy};
    ///
    /// const CUSTOM_DOMAIN_ID: usize = 42;
    /// static CUSTOM_DOMAIN: Domain<CUSTOM_DOMAIN_ID> = Domain::new(ReclaimStrategy::Eager);
    ///
    /// let atom_box = AtomBox::new_with_domain("Hello World", &CUSTOM_DOMAIN);
    /// assert_eq!(*atom_box.load(), "Hello World");
    /// ```
    pub fn new_with_domain(value: T, domain: &'domain Domain<DOMAIN_ID>) -> Self {
        let ptr = AtomicPtr::new(Box::into_raw(Box::new(value)));
        Self { ptr, domain }
    }

    /// Loads the value stored in the `AtomBox`.
    ///
    /// Returns a `LoadGuard` which can be dereferenced into the value.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new("Hello World");
    ///
    /// let value = atom_box.load();
    /// assert_eq!(*value, "Hello World");
    /// ```
    pub fn load(&self) -> LoadGuard<'domain, T, DOMAIN_ID> {
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
        LoadGuard {
            ptr,
            domain: self.domain,
            haz_ptr: Some(haz_ptr),
        }
    }

    /// Stores a new value in the `AtomBox`
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new("Hello");
    /// atom_box.store("World");
    ///
    /// let value = atom_box.load();
    /// assert_eq!(*value, "World");
    /// ```
    pub fn store(&self, value: T) {
        let _ = self.swap(value);
    }

    /// Stores the value protected by the `StoreGuard` in the `AtomBox`
    ///
    /// # Panics
    ///
    /// Panics if the guard is associated with a different domain.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box1 = AtomBox::new("Hello");
    /// let atom_box2 = AtomBox::new("World");
    ///
    /// let guard = atom_box1.swap("Bye Bye");
    ///
    /// atom_box2.store_from_guard(guard);
    /// let value = atom_box2.load();
    /// assert_eq!(*value, "Hello");
    /// ```
    pub fn store_from_guard(&self, value: StoreGuard<'domain, T, DOMAIN_ID>) {
        let _ = self.swap_from_guard(value);
    }

    /// Stores the value into the `AtomBox` and returns a `StoreGuard` which dereferences into the
    /// previous value.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new("Hello World");
    ///
    /// let guard = atom_box.swap("Bye Bye");
    /// assert_eq!(*guard, "Hello World");
    /// ```
    pub fn swap(&self, new_value: T) -> StoreGuard<'domain, T, DOMAIN_ID> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        StoreGuard {
            ptr: old_ptr,
            domain: self.domain,
        }
    }

    /// Stores the value into the `AtomBox` and returns a `StoreGuard` which dereferences into the
    /// previous value.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Panics
    ///
    /// Panics if the guard is associated with a different domain.
    ///
    /// # Example
    ///
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box1 = AtomBox::new("Hello");
    /// let atom_box2 = AtomBox::new("World");
    ///
    /// let guard1 = atom_box1.swap("Bye Bye");
    ///
    /// let guard2 = atom_box2.swap_from_guard(guard1);
    /// assert_eq!(*guard2, "World");
    /// ```
    ///
    /// The following example will fail to compile.
    ///
    /// ```compile_fail
    /// use atom_box::{AtomBox, domain::{Domain, ReclaimStrategy}};
    ///
    /// const CUSTOM_DOMAIN_ID: usize = 42;
    /// static CUSTOM_DOMAIN: Domain<CUSTOM_DOMAIN_ID> = Domain::new(ReclaimStrategy::Eager);
    ///
    /// let atom_box1 = AtomBox::new_with_domain("Hello", &CUSTOM_DOMAIN);
    /// let atom_box2 = AtomBox::new("World");
    ///
    /// let guard = atom_box1.swap("Bye bye");
    /// atom_box2.swap_from_guard(guard);
    /// ```
    pub fn swap_from_guard(
        &self,
        new_value: StoreGuard<'domain, T, DOMAIN_ID>,
    ) -> StoreGuard<'domain, T, DOMAIN_ID> {
        assert!(
            std::ptr::eq(new_value.domain, self.domain),
            "Cannot use guarded value from different domain"
        );

        let new_ptr = new_value.ptr;
        std::mem::forget(new_value);
        let old_ptr = self.ptr.swap(new_ptr as *mut T, Ordering::AcqRel);
        StoreGuard {
            ptr: old_ptr,
            domain: self.domain,
        }
    }

    /// Stores a value into the `AtomBox` if its current value equals `current_value`.
    ///
    /// The return value is a result indicating whether the new value was written.
    /// On success, this value is guaranteed to be equal to `current_value` and the return value is
    /// a StoreGuard which dereferences to the old value.
    /// On failure, the `Err` contains a LoadGaurd which dereferences to the `current_value`.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Example
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new(0);
    /// let mut current_value = atom_box.load();
    /// let initial_value = *current_value;
    /// let _ = loop {
    ///     let new_value = *current_value + 1;
    ///     match atom_box.compare_exchange(current_value, new_value) {
    ///         Ok(value) => {
    ///             break value;
    ///         }
    ///         Err(value) => {
    ///             current_value = value;
    ///         }
    ///     }
    /// };
    /// let new_value = atom_box.load();
    /// assert!(
    ///     *new_value > initial_value,
    ///     "Value should have been increased"
    /// );
    /// ```
    pub fn compare_exchange(
        &self,
        current_value: LoadGuard<'domain, T, DOMAIN_ID>,
        new_value: T,
    ) -> Result<StoreGuard<'domain, T, DOMAIN_ID>, LoadGuard<'domain, T, DOMAIN_ID>> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        match self.ptr.compare_exchange(
            current_value.ptr as *mut T,
            new_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(ptr) => Ok(StoreGuard {
                ptr,
                domain: self.domain,
            }),
            Err(ptr) => Err(LoadGuard {
                ptr,
                domain: self.domain,
                haz_ptr: None,
            }),
        }
    }

    /// Stores a value into the `AtomBox` if its current value equals `current_value`.
    ///
    /// The return value is a result indicating whether the new value was written.
    /// On success, this value is guaranteed to be equal to `current_value` and the return value is
    /// a StoreGuard which dereferences to the old value.
    /// On failure, the `Err` contains a LoadGaurd which dereferences to the `current_value`.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Panics
    ///
    /// Panics if the guard is associated with a different domain.
    ///
    /// # example
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box1 = AtomBox::new(0);
    /// let atom_box2 = AtomBox::new(1);
    ///
    /// let mut guard = atom_box2.swap(2);
    /// let mut current_value = atom_box1.load();
    /// let _ = loop {
    ///     match atom_box1.compare_exchange_from_guard(current_value, guard) {
    ///         Ok(value) => {
    ///             break value;
    ///         }
    ///         Err((value, returned_guard)) => {
    ///             current_value = value;
    ///             guard = returned_guard;
    ///         }
    ///     }
    /// };
    /// let new_value = atom_box1.load();
    /// assert!(
    ///     *new_value == 1,
    ///     "value should have been increased"
    /// );
    /// ```
    ///
    /// The following example will fail to compile.
    ///
    /// ```compile_fail
    /// use atom_box::{AtomBox, domain::{domain, reclaimstrategy}};
    ///
    /// const custom_domain_id: usize = 42;
    /// static custom_domain: domain<custom_domain_id> = domain::new(reclaimstrategy::eager);
    ///
    /// let atom_box1 = AtomBox::new_with_domain("hello", &custom_domain);
    /// let atom_box2 = AtomBox::new("world");
    ///
    /// let guard = atom_box1.swap("bye bye");
    /// let current_value = atom_box2.load();
    /// let _ = atom_box2.compare_exchange_from_guard(current_value, guard);
    /// ```
    pub fn compare_exchange_from_guard(
        &self,
        current_value: LoadGuard<'domain, T, DOMAIN_ID>,
        new_value: StoreGuard<'domain, T, DOMAIN_ID>,
    ) -> Result<
        StoreGuard<'domain, T, DOMAIN_ID>,
        (
            LoadGuard<'domain, T, DOMAIN_ID>,
            StoreGuard<'domain, T, DOMAIN_ID>,
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
                Ok(StoreGuard {
                    ptr,
                    domain: self.domain,
                })
            }
            Err(ptr) => Err((
                LoadGuard {
                    ptr,
                    domain: self.domain,
                    haz_ptr: None,
                },
                new_value,
            )),
        }
    }

    /// Stores a value into the `AtomBox` if the current value is the same as the `current` value.
    ///
    /// Unlike [`AtomBox::compare_exchange`], this function is allowed to spuriously fail even when the
    /// comparison succeeds, which can result in more efficient code on some platforms. The
    /// return value is a result indicating whether the new value was written and containing the
    /// previous value.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Example
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box = AtomBox::new(0);
    /// let mut current_value = atom_box.load();
    /// let initial_value = *current_value;
    /// let _ = loop {
    ///     let new_value = *current_value + 1;
    ///     match atom_box.compare_exchange_weak(current_value, new_value) {
    ///         Ok(value) => {
    ///             break value;
    ///         }
    ///         Err(value) => {
    ///             current_value = value;
    ///         }
    ///     }
    /// };
    /// let new_value = atom_box.load();
    /// assert!(
    ///     *new_value > initial_value,
    ///     "Value should have been increased"
    /// );
    /// ```
    pub fn compare_exchange_weak(
        &self,
        current_value: LoadGuard<'domain, T, DOMAIN_ID>,
        new_value: T,
    ) -> Result<StoreGuard<'domain, T, DOMAIN_ID>, LoadGuard<'domain, T, DOMAIN_ID>> {
        let new_ptr = Box::into_raw(Box::new(new_value));
        match self.ptr.compare_exchange_weak(
            current_value.ptr as *mut T,
            new_ptr,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(ptr) => Ok(StoreGuard {
                ptr,
                domain: self.domain,
            }),
            Err(ptr) => Err(LoadGuard {
                ptr,
                domain: self.domain,
                haz_ptr: None,
            }),
        }
    }

    /// Stores a value into the `AtomBox` if the current value is the same as the `current` value.
    ///
    /// Unlike [`AtomBox::compare_exchange_from_guard`], this function is allowed to spuriously fail even when the
    /// comparison succeeds, which can result in more efficient code on some platforms. The
    /// return value is a result indicating whether the new value was written and containing the
    /// previous value.
    ///
    /// **Note:** This method is only available on platforms that support atomic operations on
    /// pointers.
    ///
    /// # Panics
    ///
    /// Panics if the guard is associated with a different domain.
    ///
    /// # example
    /// ```
    /// use atom_box::AtomBox;
    ///
    /// let atom_box1 = AtomBox::new(0);
    /// let atom_box2 = AtomBox::new(1);
    ///
    /// let mut guard = atom_box2.swap(2);
    /// let mut current_value = atom_box1.load();
    /// let _ = loop {
    ///     match atom_box1.compare_exchange_weak_from_guard(current_value, guard) {
    ///         Ok(value) => {
    ///             break value;
    ///         }
    ///         Err((value, returned_guard)) => {
    ///             current_value = value;
    ///             guard = returned_guard;
    ///         }
    ///     }
    /// };
    /// let new_value = atom_box1.load();
    /// assert!(
    ///     *new_value == 1,
    ///     "value should have been increased"
    /// );
    /// ```
    ///
    /// The following example will fail to compile.
    ///
    /// ```compile_fail
    /// use atom_box::{AtomBox, domain::{domain, reclaimstrategy}};
    ///
    /// const custom_domain_id: usize = 42;
    /// static custom_domain: domain<custom_domain_id> = domain::new(reclaimstrategy::eager);
    ///
    /// let atom_box1 = AtomBox::new_with_domain("hello", &custom_domain);
    /// let atom_box2 = AtomBox::new("world");
    ///
    /// let guard = atom_box1.swap("bye bye");
    /// let current_value = atom_box2.load();
    /// let _ = atom_box2.compare_exchange_weak_from_guard(current_value, guard);
    /// ```
    pub fn compare_exchange_weak_from_guard(
        &self,
        current_value: LoadGuard<'domain, T, DOMAIN_ID>,
        new_value: StoreGuard<'domain, T, DOMAIN_ID>,
    ) -> Result<
        StoreGuard<'domain, T, DOMAIN_ID>,
        (
            LoadGuard<'domain, T, DOMAIN_ID>,
            StoreGuard<'domain, T, DOMAIN_ID>,
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
                Ok(StoreGuard {
                    ptr,
                    domain: self.domain,
                })
            }
            Err(ptr) => Err((
                LoadGuard {
                    ptr,
                    domain: self.domain,
                    haz_ptr: None,
                },
                new_value,
            )),
        }
    }
}

/// Contains a reference to a value that was previously contained in an `AtomBox`.
///
/// Returned from the store methods method on `AtomBox`. This value can be passed to the
/// `from_guard` methods to store this value in an `AtomBox` associated with the same domain.
///
/// Dereferences to the value.
pub struct StoreGuard<'domain, T, const DOMAIN_ID: usize> {
    ptr: *const T,
    domain: &'domain Domain<DOMAIN_ID>,
}

impl<T, const DOMAIN_ID: usize> Deref for StoreGuard<'_, T, DOMAIN_ID> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // # Safety
        //
        // The pointer is protected by the hazard pointer so will not have been dropped
        // The pointer was created via a Box so is aligned and there are no mutable references
        // since we do not give any out.
        unsafe { self.ptr.as_ref().expect("Non null") }
    }
}

impl<T, const DOMAIN_ID: usize> Drop for StoreGuard<'_, T, DOMAIN_ID> {
    fn drop(&mut self) {
        // # Safety
        //
        // The pointer to this object was originally created via box into raw.
        // The heap allocated value cannot be dropped via external code.
        // We are the only person with this pointer in a store guard. There might
        // be other people referencing it as a read only value where it is protected
        // via hazard pointers.
        // We are safe to flag it for retire, where it will be reclaimed when it is no longer
        // protected by any hazard pointers.
        unsafe { self.domain.retire(self.ptr as *mut T) };
    }
}

/// Contains a reference to a value that was stored in a `AtomBox`.
///
/// Returned as the result of calling [`AtomBox::load`].
///
/// The value is guaranteed not to be dropped before this guard is dropped.
///
/// Dereferences to the value.
pub struct LoadGuard<'domain, T, const DOMAIN_ID: usize> {
    ptr: *const T,
    // TODO: Can we remove this reference to the domain and still associate the Guard with its
    // lifetime?
    #[allow(dead_code)]
    domain: &'domain Domain<DOMAIN_ID>,
    haz_ptr: Option<&'domain HazPtr>,
}

impl<T, const DOMAIN_ID: usize> Drop for LoadGuard<'_, T, DOMAIN_ID> {
    fn drop(&mut self) {
        if let Some(haz_ptr) = self.haz_ptr {
            haz_ptr.reset();
            haz_ptr.release();
        }
    }
}

impl<T, const DOMAIN_ID: usize> Deref for LoadGuard<'_, T, DOMAIN_ID> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // # Safety
        //
        // The pointer is protected by the hazard pointer so will not have been dropped
        // The pointer was created via a Box so is aligned and there are no mutable references
        // since we do not give any out.
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
    fn single_thread_retire() {
        let atom_box = AtomBox::new(20);

        let value = atom_box.load();
        assert_eq!(
            *value, 20,
            "The correct values is returned when dereferencing"
        );
        assert_eq!(
            value.ptr,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire),
            "The hazard pointer is protecting the correct pointer"
        );

        {
            // Immediately retire the original value
            let guard = atom_box.swap(30);
            assert_eq!(
                guard.ptr, value.ptr,
                "The guard returned after swap contains a pointer to the old value"
            );
            let new_value = atom_box.load();
            assert_eq!(*new_value, 30, "The new value has been set correctly");
        }
        assert_eq!(
            *value, 20,
            "We are still able to access the old value as a result of the original load"
        );
        drop(value);
        let _ = atom_box.swap(40);
        let final_value = atom_box.load();
        assert_eq!(
            *final_value, 40,
            "When we load again we get a handle to the latest value"
        );
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
        assert_eq!(
            drop_count.load(Ordering::Acquire),
            0,
            "No values have been dropped yet"
        );
        assert_eq!(**value, 20, "The correct value is returned via load");
        assert_eq!(
            value.ptr as *mut usize,
            value.haz_ptr.unwrap().ptr.load(Ordering::Acquire),
            "The value is protected by the hazard pointer"
        );

        {
            // Immediately retire the original value
            let guard = atom_box.swap(DropTester {
                drop_count: &drop_count,
                value: 30,
            });
            assert_eq!(guard.ptr, value.ptr, "When we swap the value we get back a guard that contains a pointer to the old value");
            let new_value = atom_box.load();
            assert_eq!(
                **new_value, 30,
                "When we dereference the load, we get back a reference to the new value"
            );
            drop(guard);
        }
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            0,
            "Value should not be dropped while there is an active reference to it"
        );
        assert_eq!(**value, 20, "We are still able to access the original value since we have been holding a load guard");
        drop(value);
        let _ = atom_box.swap(DropTester {
            drop_count: &drop_count,
            value: 40,
        });
        let final_value = atom_box.load();
        assert_eq!(**final_value, 40, "The value has been updated");
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            2,
            "Both of the old values should now be dropped"
        );
    }

    #[test]
    fn swap_from_gaurd_test() {
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
            let guard2 = atom_box2.swap_from_guard(guard1);
            let _ = atom_box1.swap_from_guard(guard2);
            let new_value1 = atom_box1.load();
            let new_value2 = atom_box2.load();
            assert_eq!(
                **new_value1, 20,
                "The values in the boxes should have been swapped"
            );
            assert_eq!(
                **new_value2, 10,
                "The values in the boxes should have been swapped"
            );
        }
        assert_eq!(
            drop_count_for_placeholder.load(Ordering::Acquire),
            1,
            "The placeholder value should have been dropped"
        );
        assert_eq!(
            drop_count.load(Ordering::Acquire),
            0,
            "Neither of the initial values should have been dropped"
        );
    }
}
