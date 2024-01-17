#[cfg(all(loom, not(feature = "bicephany")))]
pub(crate) use loom::sync::atomic::AtomicBool;
#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU64};

#[cfg(all(feature = "std", not(loom)))]
pub(crate) use core::sync::atomic::AtomicU64;
#[cfg(not(loom))]
pub(crate) use core::sync::atomic::{AtomicIsize, AtomicPtr};

#[cfg(all(not(loom), not(feature = "bicephany")))]
pub(crate) use core::sync::atomic::AtomicBool;

pub(crate) use core::sync::atomic::Ordering;
