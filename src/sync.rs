#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, AtomicU64};

#[cfg(all(feature = "std", not(loom)))]
pub(crate) use core::sync::atomic::AtomicU64;
#[cfg(not(loom))]
pub(crate) use core::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr};

pub(crate) use core::sync::atomic::Ordering;
