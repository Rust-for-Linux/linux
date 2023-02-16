// SPDX-License-Identifier: GPL-2.0

//! Synchronisation primitives where access to their contents can be revoked at runtime.

use crate::{
    str::CStr,
    sync::{Guard, Lock, LockClassKey, LockFactory, LockInfo, NeedsLockClass, ReadLock, WriteLock},
    types::True,
};
use core::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    pin::Pin,
};

/// The state within the revocable synchronisation primitive.
///
/// We don't use simply `Option<T>` because we need to drop in-place because the contents are
/// implicitly pinned.
///
/// # Invariants
///
/// The `is_available` field determines if `data` is initialised.
pub struct Inner<T> {
    is_available: bool,
    data: MaybeUninit<T>,
}

impl<T> Inner<T> {
    fn new(data: T) -> Self {
        // INVARIANT: `data` is initialised and `is_available` is `true`, so the state matches.
        Self {
            is_available: true,
            data: MaybeUninit::new(data),
        }
    }

    fn drop_in_place(&mut self) {
        if !self.is_available {
            // Already dropped.
            return;
        }

        // INVARIANT: `data` is being dropped and `is_available` is set to `false`, so the state
        // matches.
        self.is_available = false;

        // SAFETY: By the type invariants, `data` is valid because `is_available` was true.
        unsafe { self.data.assume_init_drop() };
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        self.drop_in_place();
    }
}

/// Revocable synchronisation primitive.
///
/// That is, it wraps synchronisation primitives so that access to their contents can be revoked at
/// runtime, rendering them inacessible.
///
/// Once access is revoked and all concurrent users complete (i.e., all existing instances of
/// [`RevocableGuard`] are dropped), the wrapped object is also dropped.
///
/// For better ergonomics, we advise the use of specialisations of this struct, for example,
/// [`super::RevocableMutex`] and [`super::RevocableRwSemaphore`]. Callers that do not need to
/// sleep while holding on to a guard should use [`crate::revocable::Revocable`] instead, which is
/// more efficient as it uses RCU to keep objects alive.
///
/// # Examples
///
/// ```
/// # use kernel::sync::{Mutex, Revocable};
/// # use kernel::revocable_init;
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn add_two(v: &Revocable<Mutex<()>, Example>) -> Option<u32> {
///     let mut guard = v.try_write()?;
///     guard.a += 2;
///     guard.b += 2;
///     Some(guard.a + guard.b)
/// }
///
/// // SAFETY: We call `revocable_init` immediately below.
/// let mut v = unsafe { Revocable::<Mutex<()>, Example>::new(Example { a: 10, b: 20 }) };
/// // SAFETY: We never move out of `v`.
/// let pinned = unsafe { Pin::new_unchecked(&mut v) };
/// revocable_init!(pinned, "example::v");
/// assert_eq!(add_two(&v), Some(34));
/// v.revoke();
/// assert_eq!(add_two(&v), None);
/// ```
pub struct Revocable<F: LockFactory, T> {
    inner: F::LockedType<Inner<T>>,
}

/// Safely initialises a [`Revocable`] instance with the given name, generating a new lock class.
#[macro_export]
macro_rules! revocable_init {
    ($mutex:expr, $name:literal) => {
        $crate::init_with_lockdep!($mutex, $name)
    };
}

impl<F: LockFactory, T> Revocable<F, T> {
    /// Creates a new revocable instance of the given lock.
    ///
    /// # Safety
    ///
    /// The caller must call [`Revocable::init`] before using the revocable synch primitive.
    pub unsafe fn new(data: T) -> Self {
        Self {
            // SAFETY: The safety requirements of this function require that `Revocable::init`
            // be called before the returned object can be used. Lock initialisation is called
            // from `Revocable::init`.
            inner: unsafe { F::new_lock(Inner::new(data)) },
        }
    }
}

impl<F: LockFactory, T> NeedsLockClass for Revocable<F, T>
where
    F::LockedType<Inner<T>>: NeedsLockClass,
{
    fn init(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key1: &'static LockClassKey,
        key2: &'static LockClassKey,
    ) {
        // SAFETY: `inner` is pinned when `self` is.
        let inner = unsafe { self.map_unchecked_mut(|r| &mut r.inner) };
        inner.init(name, key1, key2);
    }
}

impl<F: LockFactory, T> Revocable<F, T>
where
    F::LockedType<Inner<T>>: Lock<Inner = Inner<T>>,
{
    /// Revokes access to and drops the wrapped object.
    ///
    /// Revocation and dropping happen after ongoing accessors complete.
    pub fn revoke(&self) {
        self.lock().drop_in_place();
    }

    /// Tries to lock the \[revocable\] wrapped object in write (exclusive) mode.
    ///
    /// Returns `None` if the object has been revoked and is therefore no longer accessible.
    ///
    /// Returns a guard that gives access to the object otherwise; the object is guaranteed to
    /// remain accessible while the guard is alive. Callers are allowed to sleep while holding on
    /// to the returned guard.
    pub fn try_write(&self) -> Option<RevocableGuard<'_, F, T, WriteLock>> {
        let inner = self.lock();
        if !inner.is_available {
            return None;
        }
        Some(RevocableGuard::new(inner))
    }

    fn lock(&self) -> Guard<'_, F::LockedType<Inner<T>>> {
        let ctx = self.inner.lock_noguard();
        // SAFETY: The lock was acquired in the call above.
        unsafe { Guard::new(&self.inner, ctx) }
    }
}

impl<F: LockFactory, T> Revocable<F, T>
where
    F::LockedType<Inner<T>>: Lock<ReadLock, Inner = Inner<T>>,
{
    /// Tries to lock the \[revocable\] wrapped object in read (shared) mode.
    ///
    /// Returns `None` if the object has been revoked and is therefore no longer accessible.
    ///
    /// Returns a guard that gives access to the object otherwise; the object is guaranteed to
    /// remain accessible while the guard is alive. Callers are allowed to sleep while holding on
    /// to the returned guard.
    pub fn try_read(&self) -> Option<RevocableGuard<'_, F, T, ReadLock>> {
        let ctx = self.inner.lock_noguard();
        // SAFETY: The lock was acquired in the call above.
        let inner = unsafe { Guard::new(&self.inner, ctx) };
        if !inner.is_available {
            return None;
        }
        Some(RevocableGuard::new(inner))
    }
}

/// A guard that allows access to a revocable object and keeps it alive.
pub struct RevocableGuard<'a, F: LockFactory, T, I: LockInfo>
where
    F::LockedType<Inner<T>>: Lock<I, Inner = Inner<T>>,
{
    guard: Guard<'a, F::LockedType<Inner<T>>, I>,
}

impl<'a, F: LockFactory, T, I: LockInfo> RevocableGuard<'a, F, T, I>
where
    F::LockedType<Inner<T>>: Lock<I, Inner = Inner<T>>,
{
    fn new(guard: Guard<'a, F::LockedType<Inner<T>>, I>) -> Self {
        Self { guard }
    }
}

impl<F: LockFactory, T, I: LockInfo<Writable = True>> RevocableGuard<'_, F, T, I>
where
    F::LockedType<Inner<T>>: Lock<I, Inner = Inner<T>>,
{
    /// Returns a pinned mutable reference to the wrapped object.
    pub fn as_pinned_mut(&mut self) -> Pin<&mut T> {
        // SAFETY: Revocable mutexes must be pinned, so we choose to always project the data as
        // pinned as well (i.e., we guarantee we never move it).
        unsafe { Pin::new_unchecked(&mut *self) }
    }
}

impl<F: LockFactory, T, I: LockInfo> Deref for RevocableGuard<'_, F, T, I>
where
    F::LockedType<Inner<T>>: Lock<I, Inner = Inner<T>>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.guard.data.as_ptr() }
    }
}

impl<F: LockFactory, T, I: LockInfo<Writable = True>> DerefMut for RevocableGuard<'_, F, T, I>
where
    F::LockedType<Inner<T>>: Lock<I, Inner = Inner<T>>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.guard.data.as_mut_ptr() }
    }
}
