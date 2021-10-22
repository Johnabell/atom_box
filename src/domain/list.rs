use crate::sync::{AtomicIsize, AtomicPtr, Ordering};

#[derive(Debug)]
pub(super) struct LockFreeList<T> {
    pub(super) head: AtomicPtr<Node<T>>,
    pub(super) count: AtomicIsize,
}

#[derive(Debug)]
pub(super) struct Node<T> {
    pub(super) value: T,
    pub(super) next: AtomicPtr<Node<T>>,
}

impl<T> LockFreeList<T> {
    #[cfg(not(loom))]
    pub(super) const fn new() -> Self {
        Self {
            head: AtomicPtr::new(std::ptr::null_mut()),
            count: AtomicIsize::new(0),
        }
    }

    #[cfg(loom)]
    pub(super) fn new() -> Self {
        Self {
            head: AtomicPtr::new(std::ptr::null_mut()),
            count: AtomicIsize::new(0),
        }
    }

    pub(super) fn push(&self, value: T) -> *mut Node<T> {
        let node = Box::into_raw(Box::new(Node {
            value,
            next: AtomicPtr::new(std::ptr::null_mut()),
        }));

        // # Safety
        //
        // We have ownership of T and we have just created the node so also own that.
        //
        // Since we have just created the node we are also safe to dereference it
        unsafe { self.push_all(node, &(*node).next, 1) }
    }

    // # Safety
    //
    // This function should be considered to be moving ownership of the nodes and values into this
    // list. To use this function you should adhere to the contract that you will not drop these
    // values.
    pub(super) unsafe fn push_all(
        &self,
        new_head_ptr: *mut Node<T>,
        tail_ptr: &AtomicPtr<Node<T>>,
        number_of_added_items: isize,
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
        let mut node_ptr = self.head.load(Ordering::Relaxed);
        while !node_ptr.is_null() {
            let node: Box<Node<T>> = unsafe { Box::from_raw(node_ptr) };
            node_ptr = node.next.load(Ordering::Relaxed);
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
        // # Safety
        //
        // `list2` has ownership of these values so we are considering them to be moved into list.
        // To avoid a double free we `mem::forget` `list2`
        unsafe { list.push_all(head_ptr, &(&mut *tail_node_ptr).next, 3) };

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
