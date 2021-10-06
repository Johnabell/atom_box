use super::HazPtr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

static SHARED_DOMAIN: Domain = Domain::new();

#[derive(Debug)]
pub(crate) struct Domain {
    retired: LockFreeList<*mut dyn Retirable>,
    hazard_ptrs: LockFreeList<HazPtr>,
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
    pub(crate) unsafe fn retire(&self, value: &dyn Retirable) {
        self.retired
            .push(unsafe { std::mem::transmute::<_, *mut (dyn Retirable + 'static)>(value) });
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
        let node = unsafe { &mut *node_ptr };
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
    }
}
