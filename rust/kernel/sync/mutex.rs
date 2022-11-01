// SPDX-License-Identifier: GPL-2.0

//! A kernel mutex.
//!
//! This module allows Rust code to use the kernel's [`struct mutex`].

use super::{Guard, Lock, LockClassKey, LockFactory, WriteLock};
use crate::{
    bindings,
    init::{self, PinInit},
    macros::pin_project,
    pin_init,
    str::CStr,
    Opaque,
};
use core::{cell::UnsafeCell, marker::PhantomPinned};

/// Safely initialises a [`Mutex`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! new_mutex {
    ($value:expr, $name:literal) => {
        $crate::new_with_lockdep!($crate::sync::Mutex<_>, $name, $value)
    };
}

/// Exposes the kernel's [`struct mutex`]. When multiple threads attempt to lock the same mutex,
/// only one at a time is allowed to progress, the others will block (sleep) until the mutex is
/// unlocked, at which point another thread will be allowed to wake up and make progress.
///
/// A [`Mutex`] is created using the [initialization API][init]. You can either call the `new`
/// function or use the [`new_mutex!`] macro which automatically creates the [`LockClassKey`] for you.
///
/// Since it may block, [`Mutex`] needs to be used with care in atomic contexts.
///
/// [`struct mutex`]: ../../../include/linux/mutex.h
/// [init]: ../init/index.html
#[pin_project]
pub struct Mutex<T: ?Sized> {
    /// The kernel `struct mutex` object.
    #[pin]
    mutex: Opaque<bindings::mutex>,

    /// A mutex needs to be pinned because it contains a [`struct list_head`] that is
    /// self-referential, so it cannot be safely moved once it is initialised.
    #[pin]
    _pin: PhantomPinned,

    /// The data protected by the mutex.
    data: UnsafeCell<T>,
}

// SAFETY: `Mutex` can be transferred across thread boundaries iff the data it protects can.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

// SAFETY: `Mutex` serialises the interior mutability it provides, so it is `Sync` as long as the
// data it protects is `Send`.
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Constructs a new mutex.
    #[allow(clippy::new_ret_no_self)]
    pub const fn new(
        data: T,
        name: &'static CStr,
        key: &'static LockClassKey,
    ) -> impl PinInit<Self> {
        MutexInit { data, name, key }
    }
}

#[doc(hidden)]
pub struct MutexInit<T> {
    name: &'static CStr,
    key: &'static LockClassKey,
    data: T,
}

unsafe impl<T> PinInit<Mutex<T>> for MutexInit<T> {
    unsafe fn __pinned_init(
        self,
        slot: *mut Mutex<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        let init = pin_init!(Mutex<T> {
            // SAFETY: __mutex_init is an initializer function and name and key are valid
            // parameters.
            mutex: unsafe {
                init::common::ffi_init2(
                    bindings::__mutex_init,
                    self.name.as_char_ptr(),
                    self.key.get(),
                )
            },
            data: UnsafeCell::new(self.data),
            _pin: PhantomPinned,
        });
        // SAFETY: we are inside of an initializer
        unsafe { init.__pinned_init(slot) }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Locks the mutex and gives the caller access to the data protected by it. Only one thread at
    /// a time is allowed to access the protected data.
    pub fn lock(&self) -> Guard<'_, Self> {
        let ctx = self.lock_noguard();
        // SAFETY: The mutex was just acquired.
        unsafe { Guard::new(self, ctx) }
    }
}

impl<T> LockFactory for Mutex<T> {
    type LockedType<U> = Mutex<U>;
    type Error = core::convert::Infallible;
    type Init<U> = MutexInit<U>;

    fn new_lock<U>(data: U, name: &'static CStr, key: &'static LockClassKey) -> Self::Init<U> {
        MutexInit { data, name, key }
    }
}

pub struct EmptyGuardContext;

// SAFETY: The underlying kernel `struct mutex` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for Mutex<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `mutex` points to valid memory.
        unsafe { bindings::mutex_lock(self.mutex.get()) };
        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // SAFETY: The safety requirements of the function ensure that the mutex is owned by the
        // caller.
        unsafe { bindings::mutex_unlock(self.mutex.get()) };
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

/// A revocable mutex.
///
/// That is, a mutex to which access can be revoked at runtime. It is a specialisation of the more
/// generic [`super::revocable::Revocable`].
///
/// # Examples
///
/// ```
/// # use kernel::sync::RevocableMutex;
/// # use kernel::{new_revocable, stack_init};
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn read_sum(v: &RevocableMutex<Example>) -> Option<u32> {
///     let guard = v.try_write()?;
///     Some(guard.a + guard.b)
/// }
///
/// stack_init!(let v: RevocableMutex<_> = new_revocable!(Example { a: 10, b: 20 }, "example::v"));
/// let v = v.unwrap();
/// assert_eq!(read_sum(&v), Some(30));
/// v.revoke();
/// assert_eq!(read_sum(&v), None);
/// ```
pub type RevocableMutex<T> = super::revocable::Revocable<Mutex<()>, T>;

/// A guard for a revocable mutex.
pub type RevocableMutexGuard<'a, T, I = WriteLock> =
    super::revocable::RevocableGuard<'a, Mutex<()>, T, I>;
