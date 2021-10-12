use super::HazPtr;
use std::collections::HashSet;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static SHARED_DOMAIN: Domain = Domain::new();

#[derive(Debug)]
pub(crate) struct Domain {
    // TODO: consider using TraitObject
    retired: LockFreeList<Retire>,
    hazard_ptrs: LockFreeList<HazPtr>,
}

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

#[derive(Debug)]
struct HazardPtrs {
    head: AtomicPtr<HazPtr>,
    count: AtomicUsize,
}

impl Domain {
    pub(crate) const fn new() -> Self {
        Self {
            hazard_ptrs: LockFreeList::new(),
            retired: LockFreeList::new(),
        }
    }

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
        self.retired.push(Retire::new(value));
        if self.should_reclaim() {
            self.bulk_reclaim();
        }
    }

    fn should_reclaim(&self) -> bool {
        // TODO: implement better heuristic
        true
    }

    fn bulk_reclaim(&self) -> usize {
        let retired_list = self
            .retired
            .head
            .swap(std::ptr::null_mut(), Ordering::Acquire);
        self.retired.count.store(0, Ordering::Release);
        if retired_list.is_null() {
            return 0;
        }
        let guarded_ptrs = self.get_guarded_ptrs();
        let reclaimed = self.reclaim_unguarded(guarded_ptrs, retired_list);
        reclaimed
    }

    fn reclaim_unguarded(
        &self,
        guarded_ptrs: HashSet<*mut usize>,
        retired_list: *mut Node<Retire>,
    ) -> usize {
        let mut node_ptr = retired_list;
        let mut still_retired = std::ptr::null_mut();
        let mut tail_ptr = None;
        let mut reclaimed = 0;
        let mut number_remaining = 0;
        println!("Begining reclaim");
        while !node_ptr.is_null() {
            // # Safety
            //
            // We have exclusive access to the list of reired pointers.
            let node = unsafe { &*node_ptr };
            let next = node.next.load(Ordering::Relaxed);
            // TODO: fix bug
            if guarded_ptrs.contains(&(node.value.ptr)) {
                // The pointer is still guarded keep in the retired list
                println!("Pointer gaurded");
                node.next.store(still_retired, Ordering::Relaxed);
                still_retired = node_ptr;
                if tail_ptr.is_none() {
                    tail_ptr = Some(&node.next);
                }
                number_remaining += 1;
            } else {
                println!("Pointer being freed");
                // Dealocate the retired item
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
            self.retired.push_all(still_retired, tail, number_remaining);
        }

        reclaimed
    }

    fn get_guarded_ptrs(&self) -> HashSet<*mut usize> {
        let mut guarded_ptrs = HashSet::new();
        let mut node_ptr = self.hazard_ptrs.head.load(Ordering::Acquire);
        while !node_ptr.is_null() {
            // # Safety
            //
            // Hazard pointers are only dealocated when the domain is droped
            let node = unsafe { &*node_ptr };
            if node.value.active.load(Ordering::Acquire) {
                guarded_ptrs.insert(node.value.ptr.load(Ordering::Acquire));
            }
            node_ptr = node.next.load(Ordering::Acquire);
        }
        guarded_ptrs
    }
}

impl Drop for Domain {
    fn drop(&mut self) {
        self.bulk_reclaim();
        assert!(self.retired.head.get_mut().is_null());
    }
}

pub(crate) trait Retirable {}

impl<T> Retirable for T {}

#[derive(Debug)]
struct LockFreeList<T> {
    head: AtomicPtr<Node<T>>,
    count: AtomicUsize,
}

#[derive(Debug)]
struct Node<T> {
    value: T,
    next: AtomicPtr<Node<T>>,
}

impl<T> LockFreeList<T> {
    const fn new() -> Self {
        Self {
            head: AtomicPtr::new(std::ptr::null_mut()),
            count: AtomicUsize::new(0),
        }
    }

    fn push(&self, value: T) -> *mut Node<T> {
        let node = Box::into_raw(Box::new(Node {
            value,
            next: AtomicPtr::new(std::ptr::null_mut()),
        }));
        self.push_all(node, &unsafe { &mut *node }.next, 1)
    }

    fn push_all(
        &self,
        new_head_ptr: *mut Node<T>,
        tail_ptr: &AtomicPtr<Node<T>>,
        number_of_added_items: usize,
    ) -> *mut Node<T> {
        let mut head_ptr = self.head.load(Ordering::Acquire);
        loop {
            // Safety: we currently had exclused access to the node we have just created
            tail_ptr.store(head_ptr, Ordering::Release);
            match self.head.compare_exchange_weak(
                head_ptr,
                new_head_ptr,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.count
                        .fetch_add(number_of_added_items, Ordering::Release);
                    break new_head_ptr;
                }
                Err(new_head_ptr) => {
                    head_ptr = new_head_ptr;
                }
            }
        }
    }
}

impl<T> Drop for Node<T> {
    fn drop(&mut self) {
        println!("Dropping node");
    }
}

impl<T> Drop for LockFreeList<T> {
    fn drop(&mut self) {
        println!("Dropping list");
        let mut node_ptr = *self.head.get_mut();
        while !node_ptr.is_null() {
            let mut node: Box<Node<T>> = unsafe { Box::from_raw(node_ptr) };
            node_ptr = *node.next.get_mut();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_push() {
        // Arrange
        let list = LockFreeList::new();
        // Act
        let node_ptr = list.push(1);
        // Assert
        assert_eq!(
            list.count.load(Ordering::Acquire),
            1,
            "List should have one item"
        );
        assert_eq!(
            list.head.load(Ordering::Acquire),
            node_ptr,
            "Head of list is new node"
        );
        let node: &Node<usize> = unsafe { &*node_ptr };
        assert_eq!(node.value, 1, "Value of item in node should be 1");
        assert!(node.next.load(Ordering::Acquire).is_null());
    }

    #[test]
    fn test_push_all() {
        // Arrange
        let list = LockFreeList::new();
        list.push(1);
        list.push(1);
        list.push(1);
        list.push(1);
        let list2 = LockFreeList::new();
        let tail_node_ptr = list2.push(2);
        list2.push(2);
        let head_ptr = list2.push(2);

        // Act
        list.push_all(head_ptr, &unsafe { &mut *tail_node_ptr }.next, 3);

        // Assert
        let mut values = Vec::new();
        let mut node_ptr = list.head.load(Ordering::Acquire);
        while !node_ptr.is_null() {
            let node = unsafe { &mut *node_ptr };
            values.push(node.value);
            node_ptr = node.next.load(Ordering::Acquire);
        }
        assert_eq!(values, [2, 2, 2, 1, 1, 1, 1]);
        // To avoid dropping the nodes which we moved from list2 to list1
        std::mem::forget(list2);
    }
}
