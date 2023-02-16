// SPDX-License-Identifier: GPL-2.0

//! A kernel mutex.
//!
//! This module allows Rust code to use the kernel's [`struct mutex`].

use super::{Guard, Lock, LockClassKey, LockFactory, LockIniter, WriteLock};
use crate::{bindings, str::CStr, types::Opaque};
use core::{cell::UnsafeCell, marker::PhantomPinned, pin::Pin};

/// Safely initialises a [`Mutex`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! mutex_init {
    ($mutex:expr, $name:literal) => {
        $crate::init_with_lockdep!($mutex, $name)
    };
}

/// Exposes the kernel's [`struct mutex`]. When multiple threads attempt to lock the same mutex,
/// only one at a time is allowed to progress, the others will block (sleep) until the mutex is
/// unlocked, at which point another thread will be allowed to wake up and make progress.
///
/// A [`Mutex`] must first be initialised with a call to [`Mutex::init_lock`] before it can be
/// used. The [`mutex_init`] macro is provided to automatically assign a new lock class to a mutex
/// instance.
///
/// Since it may block, [`Mutex`] needs to be used with care in atomic contexts.
///
/// [`struct mutex`]: ../../../include/linux/mutex.h
pub struct Mutex<T: ?Sized> {
    /// The kernel `struct mutex` object.
    mutex: Opaque<bindings::mutex>,

    /// A mutex needs to be pinned because it contains a [`struct list_head`] that is
    /// self-referential, so it cannot be safely moved once it is initialised.
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
    ///
    /// # Safety
    ///
    /// The caller must call [`Mutex::init_lock`] before using the mutex.
    pub const unsafe fn new(t: T) -> Self {
        Self {
            mutex: Opaque::uninit(),
            data: UnsafeCell::new(t),
            _pin: PhantomPinned,
        }
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

    unsafe fn new_lock<U>(data: U) -> Mutex<U> {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { Mutex::new(data) }
    }
}

impl<T> LockIniter for Mutex<T> {
    fn init_lock(self: Pin<&mut Self>, name: &'static CStr, key: &'static LockClassKey) {
        unsafe { bindings::__mutex_init(self.mutex.get(), name.as_char_ptr(), key.get()) };
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
/// # use kernel::revocable_init;
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
/// // SAFETY: We call `revocable_init` immediately below.
/// let mut v = unsafe { RevocableMutex::new(Example { a: 10, b: 20 }) };
/// // SAFETY: We never move out of `v`.
/// let pinned = unsafe { Pin::new_unchecked(&mut v) };
/// revocable_init!(pinned, "example::v");
/// assert_eq!(read_sum(&v), Some(30));
/// v.revoke();
/// assert_eq!(read_sum(&v), None);
/// ```
pub type RevocableMutex<T> = super::revocable::Revocable<Mutex<()>, T>;

/// A guard for a revocable mutex.
pub type RevocableMutexGuard<'a, T, I = WriteLock> =
    super::revocable::RevocableGuard<'a, Mutex<()>, T, I>;
