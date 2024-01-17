use crate::sync::{AtomicBool, AtomicPtr, Ordering};

use super::list::LockFreeList;

#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) ptr: AtomicPtr<usize>,
    pub(crate) active: AtomicBool,
}

pub(super) type HazardPointerList = LockFreeList<Node>;

impl Node {
    pub(crate) fn reset(&self) {
        self.ptr.store(core::ptr::null_mut(), Ordering::Release);
    }

    fn try_acquire(&self) -> bool {
        let active = self.active.load(Ordering::Acquire);
        !active
            && self
                .active
                .compare_exchange(active, true, Ordering::Release, Ordering::Relaxed)
                .is_ok()
    }
    pub(crate) fn release(&self) {
        self.active.store(false, Ordering::Release);
    }
    pub(crate) fn load(&self, ordering: Ordering) -> *mut usize {
        self.ptr.load(ordering)
    }
    pub(crate) fn store(&self, value: *mut usize, ordering: Ordering) {
        self.ptr.store(value, ordering)
    }
}

impl HazardPointerList {
    pub(crate) fn get_available(&self) -> Option<&Node> {
        self.iter()
            .find(|node| !node.ptr.load(Ordering::Acquire).is_null() && node.try_acquire())
    }

    pub(crate) fn set_node_available(&self, node: &Node) {
        node.reset();
        node.release();
    }

    pub(crate) fn push_in_use(&self, ptr: AtomicPtr<usize>) -> &Node {
        &unsafe {
            &*self.push(Node {
                ptr,
                active: AtomicBool::new(true),
            })
        }
        .value
    }
}
