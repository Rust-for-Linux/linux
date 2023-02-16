// SPDX-License-Identifier: GPL-2.0

//! Revocable objects.
//!
//! The [`Revocable`] type wraps other types and allows access to them to be revoked. The existence
//! of a [`RevocableGuard`] ensures that objects remain valid.

use crate::{bindings, sync::rcu};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::Deref,
    ptr::drop_in_place,
    sync::atomic::{fence, AtomicBool, AtomicU32, Ordering},
};

/// An object that can become inaccessible at runtime.
///
/// Once access is revoked and all concurrent users complete (i.e., all existing instances of
/// [`RevocableGuard`] are dropped), the wrapped object is also dropped.
///
/// # Examples
///
/// ```
/// # use kernel::revocable::Revocable;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn add_two(v: &Revocable<Example>) -> Option<u32> {
///     let guard = v.try_access()?;
///     Some(guard.a + guard.b)
/// }
///
/// let v = Revocable::new(Example { a: 10, b: 20 });
/// assert_eq!(add_two(&v), Some(30));
/// v.revoke();
/// assert_eq!(add_two(&v), None);
/// ```
///
/// Sample example as above, but explicitly using the rcu read side lock.
///
/// ```
/// # use kernel::revocable::Revocable;
/// use kernel::sync::rcu;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn add_two(v: &Revocable<Example>) -> Option<u32> {
///     let guard = rcu::read_lock();
///     let e = v.try_access_with_guard(&guard)?;
///     Some(e.a + e.b)
/// }
///
/// let v = Revocable::new(Example { a: 10, b: 20 });
/// assert_eq!(add_two(&v), Some(30));
/// v.revoke();
/// assert_eq!(add_two(&v), None);
/// ```
pub struct Revocable<T> {
    is_available: AtomicBool,
    data: MaybeUninit<UnsafeCell<T>>,
}

// SAFETY: `Revocable` is `Send` if the wrapped object is also `Send`. This is because while the
// functionality exposed by `Revocable` can be accessed from any thread/CPU, it is possible that
// this isn't supported by the wrapped object.
unsafe impl<T: Send> Send for Revocable<T> {}

// SAFETY: `Revocable` is `Sync` if the wrapped object is both `Send` and `Sync`. We require `Send`
// from the wrapped object as well because  of `Revocable::revoke`, which can trigger the `Drop`
// implementation of the wrapped object from an arbitrary thread.
unsafe impl<T: Sync + Send> Sync for Revocable<T> {}

impl<T> Revocable<T> {
    /// Creates a new revocable instance of the given data.
    pub const fn new(data: T) -> Self {
        Self {
            is_available: AtomicBool::new(true),
            data: MaybeUninit::new(UnsafeCell::new(data)),
        }
    }

    /// Tries to access the \[revocable\] wrapped object.
    ///
    /// Returns `None` if the object has been revoked and is therefore no longer accessible.
    ///
    /// Returns a guard that gives access to the object otherwise; the object is guaranteed to
    /// remain accessible while the guard is alive. In such cases, callers are not allowed to sleep
    /// because another CPU may be waiting to complete the revocation of this object.
    pub fn try_access(&self) -> Option<RevocableGuard<'_, T>> {
        let guard = rcu::read_lock();
        if self.is_available.load(Ordering::Relaxed) {
            // SAFETY: Since `self.is_available` is true, data is initialised and has to remain
            // valid because the RCU read side lock prevents it from being dropped.
            Some(unsafe { RevocableGuard::new(self.data.assume_init_ref().get(), guard) })
        } else {
            None
        }
    }

    /// Tries to access the \[revocable\] wrapped object.
    ///
    /// Returns `None` if the object has been revoked and is therefore no longer accessible.
    ///
    /// Returns a shared reference to the object otherwise; the object is guaranteed to
    /// remain accessible while the rcu read side guard is alive. In such cases, callers are not
    /// allowed to sleep because another CPU may be waiting to complete the revocation of this
    /// object.
    pub fn try_access_with_guard<'a>(&'a self, _guard: &'a rcu::Guard) -> Option<&'a T> {
        if self.is_available.load(Ordering::Relaxed) {
            // SAFETY: Since `self.is_available` is true, data is initialised and has to remain
            // valid because the RCU read side lock prevents it from being dropped.
            Some(unsafe { &*self.data.assume_init_ref().get() })
        } else {
            None
        }
    }

    /// Revokes access to and drops the wrapped object.
    ///
    /// Access to the object is revoked immediately to new callers of [`Revocable::try_access`]. If
    /// there are concurrent users of the object (i.e., ones that called [`Revocable::try_access`]
    /// beforehand and still haven't dropped the returned guard), this function waits for the
    /// concurrent access to complete before dropping the wrapped object.
    pub fn revoke(&self) {
        if self
            .is_available
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            // SAFETY: Just an FFI call, there are no further requirements.
            unsafe { bindings::synchronize_rcu() };

            // SAFETY: We know `self.data` is valid because only one CPU can succeed the
            // `compare_exchange` above that takes `is_available` from `true` to `false`.
            unsafe { drop_in_place(self.data.assume_init_ref().get()) };
        }
    }
}

impl<T> Drop for Revocable<T> {
    fn drop(&mut self) {
        // Drop only if the data hasn't been revoked yet (in which case it has already been
        // dropped).
        if *self.is_available.get_mut() {
            // SAFETY: We know `self.data` is valid because no other CPU has changed
            // `is_available` to `false` yet, and no other CPU can do it anymore because this CPU
            // holds the only reference (mutable) to `self` now.
            unsafe { drop_in_place(self.data.assume_init_ref().get()) };
        }
    }
}

/// A guard that allows access to a revocable object and keeps it alive.
///
/// CPUs may not sleep while holding on to [`RevocableGuard`] because it's in atomic context
/// holding the RCU read-side lock.
///
/// # Invariants
///
/// The RCU read-side lock is held while the guard is alive.
pub struct RevocableGuard<'a, T> {
    data_ref: *const T,
    _rcu_guard: rcu::Guard,
    _p: PhantomData<&'a ()>,
}

impl<T> RevocableGuard<'_, T> {
    fn new(data_ref: *const T, rcu_guard: rcu::Guard) -> Self {
        Self {
            data_ref,
            _rcu_guard: rcu_guard,
            _p: PhantomData,
        }
    }
}

impl<T> Deref for RevocableGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: By the type invariants, we hold the rcu read-side lock, so the object is
        // guaranteed to remain valid.
        unsafe { &*self.data_ref }
    }
}

/// An object that can become inaccessible at runtime.
///
/// Once access is revoked and all concurrent users complete (i.e., all existing instances of
/// [`AsyncRevocableGuard`] are dropped), the wrapped object is also dropped.
///
/// Unlike [`Revocable`], [`AsyncRevocable`] does not wait for concurrent users of the wrapped
/// object to finish before [`AsyncRevocable::revoke`] completes -- thus the async qualifier. This
/// has the advantage of not requiring RCU locks or waits of any kind.
///
/// # Examples
///
/// ```
/// # use kernel::revocable::AsyncRevocable;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn add_two(v: &AsyncRevocable<Example>) -> Option<u32> {
///     let guard = v.try_access()?;
///     Some(guard.a + guard.b)
/// }
///
/// let v = AsyncRevocable::new(Example { a: 10, b: 20 });
/// assert_eq!(add_two(&v), Some(30));
/// v.revoke();
/// assert_eq!(add_two(&v), None);
/// ```
///
/// Example where revocation happens while there is a user:
///
/// ```
/// # use kernel::revocable::AsyncRevocable;
/// use core::sync::atomic::{AtomicBool, Ordering};
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// static DROPPED: AtomicBool = AtomicBool::new(false);
///
/// impl Drop for Example {
///     fn drop(&mut self) {
///         DROPPED.store(true, Ordering::Relaxed);
///     }
/// }
///
/// fn add_two(v: &AsyncRevocable<Example>) -> Option<u32> {
///     let guard = v.try_access()?;
///     Some(guard.a + guard.b)
/// }
///
/// let v = AsyncRevocable::new(Example { a: 10, b: 20 });
/// assert_eq!(add_two(&v), Some(30));
///
/// let guard = v.try_access().unwrap();
/// assert!(!v.is_revoked());
/// assert!(!DROPPED.load(Ordering::Relaxed));
/// v.revoke();
/// assert!(!DROPPED.load(Ordering::Relaxed));
/// assert!(v.is_revoked());
/// assert!(v.try_access().is_none());
/// assert_eq!(guard.a + guard.b, 30);
/// drop(guard);
/// assert!(DROPPED.load(Ordering::Relaxed));
/// ```
pub struct AsyncRevocable<T> {
    usage_count: AtomicU32,
    data: MaybeUninit<UnsafeCell<T>>,
}

// SAFETY: `AsyncRevocable` is `Send` if the wrapped object is also `Send`. This is because while
// the functionality exposed by `AsyncRevocable` can be accessed from any thread/CPU, it is
// possible that this isn't supported by the wrapped object.
unsafe impl<T: Send> Send for AsyncRevocable<T> {}

// SAFETY: `AsyncRevocable` is `Sync` if the wrapped object is both `Send` and `Sync`. We require
// `Send` from the wrapped object as well because  of `AsyncRevocable::revoke`, which can trigger
// the `Drop` implementation of the wrapped object from an arbitrary thread.
unsafe impl<T: Sync + Send> Sync for AsyncRevocable<T> {}

const REVOKED: u32 = 0x80000000;
const COUNT_MASK: u32 = !REVOKED;
const SATURATED_COUNT: u32 = REVOKED - 1;

impl<T> AsyncRevocable<T> {
    /// Creates a new asynchronously revocable instance of the given data.
    pub fn new(data: T) -> Self {
        Self {
            usage_count: AtomicU32::new(0),
            data: MaybeUninit::new(UnsafeCell::new(data)),
        }
    }

    /// Tries to access the \[revocable\] wrapped object.
    ///
    /// Returns `None` if the object has been revoked and is therefore no longer accessible.
    ///
    /// Returns a guard that gives access to the object otherwise; the object is guaranteed to
    /// remain accessible while the guard is alive.
    pub fn try_access(&self) -> Option<AsyncRevocableGuard<'_, T>> {
        loop {
            let count = self.usage_count.load(Ordering::Relaxed);

            // Fail attempt to access if the object is already revoked.
            if count & REVOKED != 0 {
                return None;
            }

            // No need to increment if the count is saturated.
            if count == SATURATED_COUNT
                || self
                    .usage_count
                    .compare_exchange(count, count + 1, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                return Some(AsyncRevocableGuard { revocable: self });
            }
        }
    }

    /// Revokes access to the protected object.
    ///
    /// Returns `true` if access has been revoked, or `false` when the object has already been
    /// revoked by a previous call to [`AsyncRevocable::revoke`].
    ///
    /// This call is non-blocking, that is, no new users of the revocable object will be allowed,
    /// but potential current users are able to continue to use it and the thread won't wait for
    /// them to finish. In such cases, the object will be dropped when the last user completes.
    pub fn revoke(&self) -> bool {
        // Set the `REVOKED` bit.
        //
        // The acquire barrier matches up with the release when decrementing the usage count.
        let prev = self.usage_count.fetch_or(REVOKED, Ordering::Acquire);
        if prev & REVOKED != 0 {
            // Another thread already revoked this object.
            return false;
        }

        if prev == 0 {
            // SAFETY: This thread just revoked the object and the usage count is zero, so the
            // object is valid and there will be no future users.
            unsafe { drop_in_place(UnsafeCell::raw_get(self.data.as_ptr())) };
        }

        true
    }

    /// Returns whether access to the object has been revoked.
    pub fn is_revoked(&self) -> bool {
        self.usage_count.load(Ordering::Relaxed) & REVOKED != 0
    }
}

impl<T> Drop for AsyncRevocable<T> {
    fn drop(&mut self) {
        let count = *self.usage_count.get_mut();
        if count != REVOKED {
            // The object hasn't been dropped yet, so we do it now.

            // This matches with the release when decrementing the usage count.
            fence(Ordering::Acquire);

            // SAFETY: Since `count` is does not indicate a count of 0 and the REVOKED bit set, the
            // object is still valid.
            unsafe { drop_in_place(UnsafeCell::raw_get(self.data.as_ptr())) };
        }
    }
}

/// A guard that allows access to a revocable object and keeps it alive.
///
/// # Invariants
///
/// The owner owns an increment on the usage count (which may have saturated it), which keeps the
/// revocable object alive.
pub struct AsyncRevocableGuard<'a, T> {
    revocable: &'a AsyncRevocable<T>,
}

impl<T> Deref for AsyncRevocableGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The type invariants guarantee that the caller owns an increment.
        unsafe { &*self.revocable.data.assume_init_ref().get() }
    }
}

impl<T> Drop for AsyncRevocableGuard<'_, T> {
    fn drop(&mut self) {
        loop {
            let count = self.revocable.usage_count.load(Ordering::Relaxed);
            let actual_count = count & COUNT_MASK;
            if actual_count == SATURATED_COUNT {
                // The count is saturated, so we won't decrement (nor do we drop the object).
                return;
            }

            if actual_count == 0 {
                // Trying to underflow the count.
                panic!("actual_count is zero");
            }

            // On success, we use release ordering, which matches with the acquire in one of the
            // places where we drop the object, namely: below, in `AsyncRevocable::revoke`, or in
            // `AsyncRevocable::drop`.
            if self
                .revocable
                .usage_count
                .compare_exchange(count, count - 1, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                if count == 1 | REVOKED {
                    // `count`  is now zero and it is revoked, so free it now.

                    // This matches with the release above (which may have happened in other
                    // threads concurrently).
                    fence(Ordering::Acquire);

                    // SAFETY: Since `count` was 1, the object is still alive.
                    unsafe { drop_in_place(UnsafeCell::raw_get(self.revocable.data.as_ptr())) };
                }

                return;
            }
        }
    }
}
