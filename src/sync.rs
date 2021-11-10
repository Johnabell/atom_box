#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU64};

#[cfg(not(loom))]
pub(crate) use std::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU64};

pub(crate) use std::sync::atomic::Ordering;
