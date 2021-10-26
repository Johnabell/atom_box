//! Domain
//!
//! A domain is a holder for hazard pointers and retired objects awaiting reclamation.
//!
//! Generally, users of this library will not need to create their own domain and will simply be
//! able to make use of the global shared domain. However, for particular use cases (for example,
//! specifying the precise reclamation strategy), custom domains might be appropriate.
//!
//! When using multiple domains in a programme care must be taken to ensure that a value from an
//! `AtomBox` associated with one `Domain` is not stored in a `AtomBox` associated with a different
//! `Domain`. To help alleviate this problem, domain is parameterised by an integer ID as a const generic.
//! If used appropriately, this can provide static verification that values of one `Domain` are not stored
//! in another.
//!
//! A runtime attempt to store a value from one `Domain` in another will result in a `panic`.
//!
//! # Example
//!
//! Creating an `AtomBox` using a custom domain.
//! ```
//! use atom_box::{AtomBox, domain::{Domain, ReclaimStrategy}};
//!
//! const CUSTOM_DOMAIN_ID: usize = 42;
//! static CUSTOM_DOMAIN: Domain<CUSTOM_DOMAIN_ID> = Domain::new(ReclaimStrategy::Eager);
//!
//! let atom_box = AtomBox::new_with_domain("Hello World", &CUSTOM_DOMAIN);
//! ```
use super::HazPtr;

mod list;
mod reclaim_strategy;

use crate::macros::conditional_const;
use crate::sync::Ordering;
use list::{LockFreeList, Node};
pub use reclaim_strategy::{ReclaimStrategy, TimedCappedSettings};
use std::collections::HashSet;

pub(crate) trait Retirable {}

impl<T> Retirable for T {}

// TODO: consider using TraitObject
#[derive(Debug)]
struct Retire {
    ptr: *mut usize,
    retirable: *mut dyn Retirable,
}

impl Retire {
    fn new<T>(ptr: *mut T) -> Self {
        Self {
            ptr: ptr as *mut usize,
            retirable: unsafe { std::mem::transmute(ptr as *mut dyn Retirable) },
        }
    }
}

/// A holder of hazard pointers protecting the access to the values stored in all associated `AtomBox`s.
///
/// A domain is responsible for handing out hazard pointer to protect the access to the values
/// stored in different `AtomBox`s.
///
/// The domain is also responsible for holding onto retired items until they can safely be
/// reclaimed.
#[derive(Debug)]
pub struct Domain<const DOMAIN_ID: usize> {
    retired: LockFreeList<Retire>,
    hazard_ptrs: LockFreeList<HazPtr>,
    reclaim_strategy: ReclaimStrategy,
}

impl<const DOMAIN_ID: usize> Domain<DOMAIN_ID> {
    #[cfg(not(loom))]
    pub(crate) const fn default() -> Self {
        Self::_new(ReclaimStrategy::default())
    }

    conditional_const!(
        "Create a new `Domain` with provided `ReclaimStrategy`.

# Example

```
use atom_box::domain::{Domain, ReclaimStrategy};

const CUSTOM_DOMAIN_ID: usize = 42;
static CUSTOM_DOMAIN: Domain<CUSTOM_DOMAIN_ID> = Domain::new(ReclaimStrategy::Eager);
```

On nightly this will panic if the domain id is equal to the shared domain's id (0).
",
        pub,
        fn new(reclaim_strategy: ReclaimStrategy) -> Self {
            // Find away to statically enforce this
            #[cfg(nightly)]
            assert!(DOMAIN_ID != crate::SHARED_DOMAIN_ID);
            Self::_new(reclaim_strategy)
        }
    );

    conditional_const!(
        "Internal function for creating a new `Domain`",
        pub(crate),
        fn _new(reclaim_strategy: ReclaimStrategy) -> Self {
            Self {
                hazard_ptrs: LockFreeList::new(),
                retired: LockFreeList::new(),
                reclaim_strategy,
            }
        }
    );

    pub(crate) fn acquire_haz_ptr(&self) -> &HazPtr {
        if let Some(haz_ptr) = self.try_acquire_haz_ptr() {
            haz_ptr
        } else {
            self.acquire_new_haz_ptr()
        }
    }

    fn try_acquire_haz_ptr(&self) -> Option<&HazPtr> {
        let mut haz_ptr = self.hazard_ptrs.head.load(Ordering::Acquire);
        while !haz_ptr.is_null() {
            let hazard = unsafe { &*haz_ptr };
            if hazard.value.try_acquire() {
                return Some(&hazard.value);
            }
            haz_ptr = hazard.next.load(Ordering::Acquire)
        }
        None
    }

    fn acquire_new_haz_ptr(&self) -> &HazPtr {
        let haz_ptr = HazPtr::new(true);
        let node = unsafe { &*self.hazard_ptrs.push(haz_ptr) };
        &node.value
    }

    /// Places a pointer on the retire list to be safely reclaimed when no hazard pointers are
    /// referencing it.
    ///
    /// # Safety
    ///
    /// Must ensure that no-one else calls retire on the same value.
    /// Value must be associated with this domain.
    /// Value must be able to live as long as the domain.
    pub(crate) unsafe fn retire<T>(&self, value: *mut T) {
        std::sync::atomic::fence(Ordering::SeqCst);

        self.retired.push(Retire::new(value));
        if self.should_reclaim() {
            self.bulk_reclaim();
        }
    }

    fn should_reclaim(&self) -> bool {
        self.reclaim_strategy.should_reclaim(
            self.retired.count.load(Ordering::Acquire),
            self.retired.count.load(Ordering::Acquire),
        )
    }

    fn bulk_reclaim(&self) -> usize {
        let retired_list = self
            .retired
            .head
            .swap(std::ptr::null_mut(), Ordering::Acquire);

        std::sync::atomic::fence(Ordering::SeqCst);

        self.retired.count.store(0, Ordering::Release);
        if retired_list.is_null() {
            return 0;
        }
        let guarded_ptrs = self.get_guarded_ptrs();
        self.reclaim_unguarded(guarded_ptrs, retired_list)
    }

    fn reclaim_unguarded(
        &self,
        guarded_ptrs: HashSet<*const usize>,
        retired_list: *mut Node<Retire>,
    ) -> usize {
        let mut node_ptr = retired_list;
        let mut still_retired = std::ptr::null_mut();
        let mut tail_ptr = None;
        let mut reclaimed = 0;
        let mut number_remaining = 0;
        while !node_ptr.is_null() {
            // # Safety
            //
            // We have exclusive access to the list of retired pointers.
            let node = unsafe { &*node_ptr };
            let next = node.next.load(Ordering::Relaxed);
            if guarded_ptrs.contains(&(node.value.ptr as *const usize)) {
                // The pointer is still guarded keep in the retired list
                node.next.store(still_retired, Ordering::Relaxed);
                still_retired = node_ptr;
                if tail_ptr.is_none() {
                    tail_ptr = Some(&node.next);
                }
                number_remaining += 1;
            } else {
                // Deallocate the retired item
                //
                // # Safety
                //
                // The value was originally allocated via a box. Therefore all the safety
                // requirement of box are met. According to the safety requirements of retire,
                // the pointer has not yet been dropped and has only been placed in the retired
                // list once. There are currently no other threads looking at the value since it is
                // no longer protected by any of the hazard pointers.
                unsafe { std::ptr::drop_in_place(node.value.retirable) };

                // # Safety
                //
                // The node was originally allocated via box, therefore, all the safety
                // requirements of box are met. We have exclusive access to the node so can
                // therefore safely drop it.
                let _node = unsafe { Box::from_raw(node_ptr) };

                reclaimed += 1;
            }
            node_ptr = next;
        }

        if let Some(tail) = tail_ptr {
            std::sync::atomic::fence(Ordering::SeqCst);

            // # Safety
            //
            // All of the nodes in this list were originally owned by the retired list. We are
            // putting them back in.
            unsafe { self.retired.push_all(still_retired, tail, number_remaining) };
        }

        reclaimed
    }

    fn get_guarded_ptrs(&self) -> HashSet<*const usize> {
        let mut guarded_ptrs = HashSet::new();
        let mut node_ptr = self.hazard_ptrs.head.load(Ordering::Acquire);
        while !node_ptr.is_null() {
            // # Safety
            //
            // Hazard pointers are only deallocated when the domain is dropped
            let node = unsafe { &*node_ptr };
            if node.value.active.load(Ordering::Acquire) {
                guarded_ptrs.insert(node.value.ptr.load(Ordering::Acquire) as *const usize);
            }
            node_ptr = node.next.load(Ordering::Acquire);
        }
        guarded_ptrs
    }
}

impl<const DOMAIN_ID: usize> Drop for Domain<DOMAIN_ID> {
    fn drop(&mut self) {
        self.bulk_reclaim();
        assert!(self.retired.head.load(Ordering::Relaxed).is_null());
    }
}
