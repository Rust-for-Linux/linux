// SPDX-License-Identifier: GPL-2.0

//! A generic lock guard and trait.
//!
//! This module contains a lock guard that can be used with any locking primitive that implements
//! the ([`Lock`]) trait. It also contains the definition of the trait, which can be leveraged by
//! other constructs to work on generic locking primitives.

use super::LockClassKey;
use crate::{init::PinInit, str::CStr, Bool, False, True};

/// Allows mutual exclusion primitives that implement the [`Lock`] trait to automatically unlock
/// when a guard goes out of scope. It also provides a safe and convenient way to access the data
/// protected by the lock.
#[must_use = "the lock unlocks immediately when the guard is unused"]
pub struct Guard<'a, L: Lock<I> + ?Sized, I: LockInfo = WriteLock> {
    pub(crate) lock: &'a L,
    pub(crate) context: L::GuardContext,
}

// SAFETY: `Guard` is sync when the data protected by the lock is also sync. This is more
// conservative than the default compiler implementation; more details can be found on
// <https://github.com/rust-lang/rust/issues/41622> -- it refers to `MutexGuard` from the standard
// library.
unsafe impl<L, I> Sync for Guard<'_, L, I>
where
    L: Lock<I> + ?Sized,
    L::Inner: Sync,
    I: LockInfo,
{
}

impl<L: Lock<I> + ?Sized, I: LockInfo> core::ops::Deref for Guard<'_, L, I> {
    type Target = L::Inner;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The caller owns the lock, so it is safe to deref the protected data.
        unsafe { &*self.lock.locked_data().get() }
    }
}

impl<L: Lock<I> + ?Sized, I: LockInfo<Writable = True>> core::ops::DerefMut for Guard<'_, L, I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The caller owns the lock, so it is safe to deref the protected data.
        unsafe { &mut *self.lock.locked_data().get() }
    }
}

impl<L: Lock<I> + ?Sized, I: LockInfo> Drop for Guard<'_, L, I> {
    fn drop(&mut self) {
        // SAFETY: The caller owns the lock, so it is safe to unlock it.
        unsafe { self.lock.unlock(&mut self.context) };
    }
}

impl<'a, L: Lock<I> + ?Sized, I: LockInfo> Guard<'a, L, I> {
    /// Constructs a new immutable lock guard.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it owns the lock.
    pub(crate) unsafe fn new(lock: &'a L, context: L::GuardContext) -> Self {
        Self { lock, context }
    }
}

/// Specifies properties of a lock.
pub trait LockInfo {
    /// Determines if the data protected by a lock is writable.
    type Writable: Bool;
}

/// A marker for locks that only allow reading.
pub struct ReadLock;
impl LockInfo for ReadLock {
    type Writable = False;
}

/// A marker for locks that allow reading and writing.
pub struct WriteLock;
impl LockInfo for WriteLock {
    type Writable = True;
}

/// A generic mutual exclusion primitive.
///
/// [`Guard`] is written such that any mutual exclusion primitive that can implement this trait can
/// also benefit from having an automatic way to unlock itself.
///
/// # Safety
///
/// - Implementers of this trait with the [`WriteLock`] marker must ensure that only one thread/CPU
///   may access the protected data once the lock is held, that is, between calls to `lock_noguard`
///   and `unlock`.
/// - Implementers of all other markers must ensure that a mutable reference to the protected data
///   is not active in any thread/CPU because at least one shared reference is active between calls
///   to `lock_noguard` and `unlock`.
pub unsafe trait Lock<I: LockInfo = WriteLock> {
    /// The type of the data protected by the lock.
    type Inner: ?Sized;

    /// The type of context, if any, that needs to be stored in the guard.
    type GuardContext;

    /// Acquires the lock, making the caller its owner.
    #[must_use]
    fn lock_noguard(&self) -> Self::GuardContext;

    /// Reacquires the lock, making the caller its owner.
    ///
    /// The guard context before the last unlock is passed in.
    ///
    /// Locks that don't require this state on relock can simply use the default implementation
    /// that calls [`Lock::lock_noguard`].
    fn relock(&self, ctx: &mut Self::GuardContext) {
        *ctx = self.lock_noguard();
    }

    /// Releases the lock, giving up ownership of the lock.
    ///
    /// # Safety
    ///
    /// It must only be called by the current owner of the lock.
    unsafe fn unlock(&self, context: &mut Self::GuardContext);

    /// Returns the data protected by the lock.
    fn locked_data(&self) -> &core::cell::UnsafeCell<Self::Inner>;
}

/// A creator of instances of a mutual exclusion (lock) primitive.
pub trait LockFactory {
    /// The parametrised type of the mutual exclusion primitive that can be created by this factory.
    type LockedType<T>;
    /// Error that can occur when creating a lock of this kind.
    type Error;
    /// the initializer used to initialize a lock of this kind.
    type Init<T>: PinInit<Self::LockedType<T>, Self::Error>;

    /// Constructs a new instance of the mutual exclusion primitive.
    fn new_lock<T>(data: T, name: &'static CStr, key: &'static LockClassKey) -> Self::Init<T>;
}
