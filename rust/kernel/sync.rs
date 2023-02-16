// SPDX-License-Identifier: GPL-2.0

//! Synchronisation primitives.
//!
//! This module contains the kernel APIs related to synchronisation that have been ported or
//! wrapped for usage by Rust code in the kernel.

mod arc;
mod condvar;
mod guard;
mod mutex;
pub mod rcu;
mod revocable;
pub mod smutex;

use crate::{bindings, str::CStr};
use core::{cell::UnsafeCell, mem::MaybeUninit, pin::Pin};

pub use arc::{Arc, ArcBorrow, UniqueArc};
pub use condvar::CondVar;
pub use guard::{Guard, Lock, LockFactory, LockInfo, LockIniter, ReadLock, WriteLock};
pub use mutex::{Mutex, RevocableMutex, RevocableMutexGuard};
pub use revocable::{Revocable, RevocableGuard};

/// Represents a lockdep class. It's a wrapper around C's `lock_class_key`.
#[repr(transparent)]
pub struct LockClassKey(UnsafeCell<MaybeUninit<bindings::lock_class_key>>);

// SAFETY: This is a wrapper around a lock class key, so it is safe to use references to it from
// any thread.
unsafe impl Sync for LockClassKey {}

impl LockClassKey {
    /// Creates a new lock class key.
    pub const fn new() -> Self {
        Self(UnsafeCell::new(MaybeUninit::uninit()))
    }

    #[allow(dead_code)]
    pub(crate) fn get(&self) -> *mut bindings::lock_class_key {
        self.0.get().cast()
    }
}

/// Safely initialises an object that has an `init` function that takes a name and a lock class as
/// arguments, examples of these are [`Mutex`] and [`SpinLock`]. Each of them also provides a more
/// specialised name that uses this macro.
#[doc(hidden)]
#[macro_export]
macro_rules! init_with_lockdep {
    ($obj:expr, $name:expr) => {{
        static CLASS1: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        static CLASS2: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        let obj = $obj;
        let name = $crate::c_str!($name);
        $crate::sync::NeedsLockClass::init(obj, name, &CLASS1, &CLASS2)
    }};
}

/// A trait for types that need a lock class during initialisation.
///
/// Implementers of this trait benefit from the [`init_with_lockdep`] macro that generates a new
/// class for each initialisation call site.
pub trait NeedsLockClass {
    /// Initialises the type instance so that it can be safely used.
    ///
    /// Callers are encouraged to use the [`init_with_lockdep`] macro as it automatically creates a
    /// new lock class on each usage.
    fn init(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key1: &'static LockClassKey,
        key2: &'static LockClassKey,
    );
}
