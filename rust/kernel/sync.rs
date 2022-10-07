// SPDX-License-Identifier: GPL-2.0

//! Synchronisation primitives.
//!
//! This module contains the kernel APIs related to synchronisation that have been ported or
//! wrapped for usage by Rust code in the kernel and is shared by all of them.
//!
//! # Examples
//!
//! ```
//! # use kernel::new_mutex;
//! # use kernel::sync::Mutex;
//! # use alloc::boxed::Box;
//! # use core::pin::Pin;
//! let data = Box::pin_init(new_mutex!(10, "test::data")).unwrap();
//!
//! assert_eq!(*data.lock(), 10);
//! *data.lock() = 20;
//! assert_eq!(*data.lock(), 20);
//! ```

use crate::{bindings, init::PinInit};
use core::{cell::UnsafeCell, mem::MaybeUninit};

mod arc;
mod condvar;
mod guard;
mod locked_by;
mod mutex;
mod nowait;
pub mod rcu;
mod revocable;
mod rwsem;
mod seqlock;
pub mod smutex;
mod spinlock;

pub use arc::{new_refcount, Arc, ArcBorrow, StaticArc, UniqueArc};
pub use condvar::CondVar;
pub use guard::{Guard, Lock, LockFactory, LockInfo, ReadLock, WriteLock};
pub use locked_by::LockedBy;
pub use mutex::{Mutex, RevocableMutex, RevocableMutexGuard};
pub use nowait::{NoWaitLock, NoWaitLockGuard};
pub use revocable::{Revocable, RevocableGuard};
pub use rwsem::{RevocableRwSemaphore, RevocableRwSemaphoreGuard, RwSemaphore};
pub use seqlock::{SeqLock, SeqLockReadGuard};
pub use spinlock::{RawSpinLock, SpinLock};

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

    pub(crate) fn get(&self) -> *mut bindings::lock_class_key {
        self.0.get().cast()
    }
}

/// Safely initialises an object that has an `init` function that takes a name and a lock class as
/// arguments, examples of these are [`Mutex`] and [`SpinLock`]. Each of them also provides a more
/// specialised name that uses this macro.
#[doc(hidden)]
#[macro_export]
macro_rules! new_with_lockdep {
    ($what:ty,  $name:expr $(, $obj:expr $(,)?)?) => {{
        static CLASS1: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        let name = $crate::c_str!($name);
        <$what>::new($($obj,)? name, &CLASS1)
    }};
}

/// Automatically initialises static instances of synchronisation primitives.
///
/// The syntax resembles that of regular static variables, except that the value assigned is that
/// of the protected type (if one exists). In the examples below, all primitives except for
/// [`CondVar`] require the inner value to be supplied.
///
/// # Examples
///
/// ```ignore
/// # use kernel::{init_static_sync, sync::{CondVar, Mutex, RevocableMutex, SpinLock}};
/// struct Test {
///     a: u32,
///     b: u32,
/// }
///
/// init_static_sync! {
///     static A: Mutex<Test> = Test { a: 10, b: 20 };
///
///     /// Documentation for `B`.
///     pub static B: Mutex<u32> = 0;
///
///     pub(crate) static C: SpinLock<Test> = Test { a: 10, b: 20 };
///     static D: CondVar;
///
///     static E: RevocableMutex<Test> = Test { a: 30, b: 40 };
/// }
/// ```
#[macro_export]
macro_rules! init_static_sync {
    ($($(#[$outer:meta])* $v:vis static $id:ident : $t:ty $(= $value:expr)?;)*) => {
        $(
            $(#[$outer])*
            $v static $id: $crate::sync::StaticInit<$t> = {
                #[link_section = ".init_array"]
                #[used]
                static TMP: extern "C" fn() = {
                    extern "C" fn constructor() {
                        // SAFETY: This locally-defined function is only called from a constructor,
                        // which guarantees that `$id` is not accessible from other threads
                        // concurrently and this is only called once.
                        unsafe { $crate::sync::StaticInit::init(&$id, $crate::new_with_lockdep!($t, stringify!($id) $(, $value)? )) };
                    }
                    constructor
                };
                // SAFETY: the initializer is called above in the ctor
                unsafe { $crate::sync::StaticInit::uninit() }
            };
        )*
    };
}

/// A statically initialized value.
pub struct StaticInit<T> {
    inner: MaybeUninit<UnsafeCell<T>>,
}

unsafe impl<T: Sync> Sync for StaticInit<T> {}
unsafe impl<T: Send> Send for StaticInit<T> {}

impl<T> StaticInit<T> {
    /// Creates a new `StaticInit` that is uninitialized.
    ///
    /// # Safety
    ///
    /// The caller calls `Self::init` exactly once before using this value.
    pub const unsafe fn uninit() -> Self {
        Self {
            inner: MaybeUninit::uninit(),
        }
    }

    /// Initializes the contents of `self`.
    ///
    /// # Safety
    ///
    /// The caller calls this function exactly once and before any other function (even implicitly
    /// derefing) of `self` is called.
    pub unsafe fn init<E>(&self, init: impl PinInit<T, E>)
    where
        E: Into<core::convert::Infallible>,
    {
        unsafe {
            let ptr = UnsafeCell::raw_get(self.inner.as_ptr());
            match init.__pinned_init(ptr).map_err(|e| e.into()) {
                Ok(()) => {}
                Err(e) => match e {},
            }
        }
    }
}

impl<T> core::ops::Deref for StaticInit<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.assume_init_ref().get() }
    }
}

/// Reschedules the caller's task if needed.
pub fn cond_resched() -> bool {
    // SAFETY: No arguments, reschedules `current` if needed.
    unsafe { bindings::cond_resched() != 0 }
}
