use crate::macros::conditional_const;
use crate::sync::{AtomicIsize, AtomicPtr, Ordering};
use alloc::boxed::Box;
use core::ops::Deref;
use core::ptr::NonNull;

#[derive(Debug)]
pub(super) struct Bicephaly<T> {
    pub(super) available_head: AtomicPtr<Node<T>>,
    pub(super) in_use_head: AtomicPtr<Node<T>>,
    pub(super) available_count: AtomicIsize,
    pub(super) in_use_count: AtomicIsize,
}

#[derive(Debug)]
pub(crate) struct Node<T> {
    pub(super) value: T,
    pub(super) next_available: AtomicPtr<Node<T>>,
    pub(super) next_in_use: AtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    conditional_const!(
        "Creates a new node for [`Bicephaly`]",
        pub(self),
        fn new(value: T) -> Self {
            Self {
                value,
                next_available: AtomicPtr::new(core::ptr::null_mut()),
                next_in_use: AtomicPtr::new(core::ptr::null_mut()),
            }
        }
    );
}

impl<T> Deref for Node<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

macro_rules! push_node_method {
    ($push_node_method_name:ident, $head:ident, $next:ident, $count:ident) => {
        /// # Safety
        ///
        /// The pointer to the node must be safe to dereference and passing a node to this function
        /// should be considered to be passing ownership of the node to the [`Bicephaly`].
        pub(super) fn $push_node_method_name(&self, node: *mut Node<T>) -> *mut Node<T> {
            // # Safety
            //
            // We have ownership of the node by vurtue of the type system.
            //
            // Since we have just created the node we are also safe to dereference it
            let mut head_ptr = self.$head.load(Ordering::Acquire);
            loop {
                // Safety: we currently have exclusive access to the node we have just created
                unsafe { &*node }.$next.store(head_ptr, Ordering::Release);
                match self.$head.compare_exchange_weak(
                    head_ptr,
                    node,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        self.$count.fetch_add(1, Ordering::Release);
                        break node;
                    }
                    Err(new_head_ptr) => {
                        head_ptr = new_head_ptr;
                    }
                }
            }
        }
    };
}

impl<T> Bicephaly<T> {
    conditional_const!(
        "Creates a new `Bicephaly`",
        pub,
        fn new() -> Self {
            Self {
                available_head: AtomicPtr::new(core::ptr::null_mut()),
                in_use_head: AtomicPtr::new(core::ptr::null_mut()),
                available_count: AtomicIsize::new(0),
                in_use_count: AtomicIsize::new(0),
            }
        }
    );

    pub(super) fn get_available(&self) -> Option<&Node<T>> {
        match self.pop_available_node() {
            None => None,
            Some(mut node) => {
                let node = unsafe { node.as_mut() };
                Some(node)
            }
        }
    }

    pub(super) fn set_node_available(&self, node: &Node<T>) {
        self.push_available_node(node as *const _ as *mut _);
    }

    pub(super) fn pop_available_node(&self) -> Option<NonNull<Node<T>>> {
        let mut head_ptr = self.available_head.load(Ordering::Acquire);
        while !head_ptr.is_null() {
            // # Safety
            //
            // We know the pointer is non null since we have just checked this. Given the
            // safety guarantees of the other methods we know that we will have a valid pointer
            // to a node since we are the only ones who can create nodes.
            let head = unsafe { &*head_ptr };
            let new_head_ptr = head.next_available.load(Ordering::Acquire);

            match self.available_head.compare_exchange_weak(
                head_ptr,
                new_head_ptr,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.available_count.fetch_add(-1, Ordering::Release);
                    return NonNull::new(head_ptr);
                }
                Err(updated_head_ptr) => {
                    head_ptr = updated_head_ptr;
                }
            }
        }
        None
    }

    pub(super) fn push_in_use(&self, value: T) -> *mut Node<T> {
        let node = Box::into_raw(Box::new(Node::new(value)));

        // # Safety
        //
        // We have ownership of T and we have just created the node so also own that.
        //
        // Since we have just created the node we are also safe to dereference it
        self.push_in_use_node(node)
    }

    push_node_method!(
        push_available_node,
        available_head,
        next_available,
        available_count
    );

    push_node_method!(push_in_use_node, in_use_head, next_in_use, in_use_count);
}

impl<T> Drop for Bicephaly<T> {
    fn drop(&mut self) {
        let mut node_ptr = self.in_use_head.load(Ordering::Relaxed);
        while !node_ptr.is_null() {
            let node: Box<Node<T>> = unsafe { Box::from_raw(node_ptr) };
            node_ptr = node.next_in_use.load(Ordering::Relaxed);
        }
    }
}

#[cfg(not(loom))]
#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_push_in_use() {
        // Arrange
        let list = Bicephaly::new();
        // Act
        let node_ptr = list.push_in_use(1);
        // Assert
        assert_eq!(
            list.in_use_count.load(Ordering::Acquire),
            1,
            "List should have one item"
        );
        assert_eq!(
            list.in_use_head.load(Ordering::Acquire),
            node_ptr,
            "Head of list is new node"
        );
        let node: &Node<usize> = unsafe { &*node_ptr };
        assert_eq!(node.value, 1, "Value of item in node should be 1");
        assert!(
            node.next_in_use.load(Ordering::Acquire).is_null(),
            "The next pointer should be null"
        );
    }

    #[test]
    fn test_pop_available_node() {
        // Arrange
        let list = Bicephaly::new();
        let node = list.push_in_use(1);
        list.set_node_available(unsafe { &*node });

        // Act
        let popped_node = list
            .pop_available_node()
            .expect("List not empty should get back a node");
        // Assert
        assert_eq!(
            list.available_count.load(Ordering::Acquire),
            0,
            "List should have no items"
        );
        assert_eq!(
            list.available_head.load(Ordering::Acquire),
            core::ptr::null_mut(),
            "List is now empty, head should be null"
        );
        let node: &Node<usize> = unsafe { popped_node.as_ref() };
        assert_eq!(node.value, 1, "Value of item in node should be 1");
        assert!(
            node.next_available.load(Ordering::Acquire).is_null(),
            "The next pointer should be null"
        );
    }
}
