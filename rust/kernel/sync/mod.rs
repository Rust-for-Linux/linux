// SPDX-License-Identifier: GPL-2.0

//! Synchronisation primitives.
//!
//! This module contains the kernel APIs related to synchronisation that have been ported or
//! wrapped for usage by Rust code in the kernel and is shared by all of them.
//!
//! # Example
//!
//! ```no_run
//! # use kernel::prelude::*;
//! # use kernel::mutex_init;
//! # use kernel::sync::Mutex;
//! # use alloc::boxed::Box;
//! # use core::pin::Pin;
//! // SAFETY: `init` is called below.
//! let mut data = Pin::from(Box::new(unsafe { Mutex::new(0) }));
//! mutex_init!(data.as_mut(), "test::data");
//! *data.lock() = 10;
//! pr_info!("{}\n", *data.lock());
//! ```

use crate::{bindings, str::CStr};
use core::pin::Pin;

mod arc;
mod condvar;
mod guard;
mod locked_by;
mod mutex;
mod revocable_mutex;
mod rwsem;
mod seqlock;
mod spinlock;

pub use arc::{Ref, RefBorrow, UniqueRef};
pub use condvar::CondVar;
pub use guard::{CreatableLock, Guard, Lock, LockInfo, ReadLock, WriteLock};
pub use locked_by::LockedBy;
pub use mutex::Mutex;
pub use revocable_mutex::{RevocableMutex, RevocableMutexGuard};
pub use rwsem::RwSemaphore;
pub use seqlock::{SeqLock, SeqLockReadGuard};
pub use spinlock::{IrqDisableSpinLock, SpinLock};

/// Safely initialises an object that has an `init` function that takes a name and a lock class as
/// arguments, examples of these are [`Mutex`] and [`SpinLock`]. Each of them also provides a more
/// specialised name that uses this macro.
#[doc(hidden)]
#[macro_export]
macro_rules! init_with_lockdep {
    ($obj:expr, $name:expr) => {{
        static mut CLASS1: core::mem::MaybeUninit<$crate::bindings::lock_class_key> =
            core::mem::MaybeUninit::uninit();
        static mut CLASS2: core::mem::MaybeUninit<$crate::bindings::lock_class_key> =
            core::mem::MaybeUninit::uninit();
        let obj = $obj;
        let name = $crate::c_str!($name);
        // SAFETY: `CLASS1` and `CLASS2` are never used by Rust code directly; the C portion of the
        // kernel may change it though.
        #[allow(unused_unsafe)]
        unsafe {
            $crate::sync::NeedsLockClass::init(obj, name, CLASS1.as_mut_ptr(), CLASS2.as_mut_ptr())
        };
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
    ///
    /// # Safety
    ///
    /// `key1` and `key2` must point to valid memory locations and remain valid until `self` is
    /// dropped.
    unsafe fn init(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key1: *mut bindings::lock_class_key,
        key2: *mut bindings::lock_class_key,
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
