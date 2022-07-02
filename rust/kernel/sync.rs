// SPDX-License-Identifier: GPL-2.0

//! Synchronisation primitives.
//!
//! This module contains the kernel APIs related to synchronisation that have been ported or
//! wrapped for usage by Rust code in the kernel and is shared by all of them.
//!
//! # Example
//!
//! ```
//! # use kernel::mutex_init;
//! # use kernel::sync::Mutex;
//! # use alloc::boxed::Box;
//! # use core::pin::Pin;
//! // SAFETY: `init` is called below.
//! let mut data = Pin::from(Box::try_new(unsafe { Mutex::new(10) }).unwrap());
//! mutex_init!(data.as_mut(), "test::data");
//!
//! assert_eq!(*data.lock(), 10);
//! *data.lock() = 20;
//! assert_eq!(*data.lock(), 20);
//! ```

use crate::{bindings, str::CStr};
use core::{cell::UnsafeCell, mem::MaybeUninit, pin::Pin};

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

pub use arc::{new_refcount, Ref, RefBorrow, StaticRef, UniqueRef};
pub use condvar::CondVar;
pub use guard::{Guard, Lock, LockFactory, LockInfo, LockIniter, ReadLock, WriteLock};
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
            $v static $id: $t = {
                #[link_section = ".init_array"]
                #[used]
                static TMP: extern "C" fn() = {
                    extern "C" fn constructor() {
                        // SAFETY: This locally-defined function is only called from a constructor,
                        // which guarantees that `$id` is not accessible from other threads
                        // concurrently.
                        #[allow(clippy::cast_ref_to_mut)]
                        let mutable = unsafe { &mut *(&$id as *const _ as *mut $t) };
                        // SAFETY: It's a shared static, so it cannot move.
                        let pinned = unsafe { core::pin::Pin::new_unchecked(mutable) };
                        $crate::init_with_lockdep!(pinned, stringify!($id));
                    }
                    constructor
                };
                $crate::init_static_sync!(@call_new $t, $($value)?)
            };
        )*
    };
    (@call_new $t:ty, $value:expr) => {{
        let v = $value;
        // SAFETY: the initialisation function is called by the constructor above.
        unsafe { <$t>::new(v) }
    }};
    (@call_new $t:ty,) => {
        // SAFETY: the initialisation function is called by the constructor above.
        unsafe { <$t>::new() }
    };
}

/// Reschedules the caller's task if needed.
pub fn cond_resched() -> bool {
    // SAFETY: No arguments, reschedules `current` if needed.
    unsafe { bindings::cond_resched() != 0 }
}
