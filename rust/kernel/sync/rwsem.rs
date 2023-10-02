// SPDX-License-Identifier: GPL-2.0

//! A kernel read/write mutex.
//!
//! This module allows Rust code to use the kernel's [`struct rw_semaphore`].
//!
//! C header: [`include/linux/rwsem.h`](../../../../include/linux/rwsem.h)

use super::{
    mutex::EmptyGuardContext, Guard, Lock, LockClassKey, LockFactory, LockIniter, ReadLock,
    WriteLock,
};
use crate::{bindings, str::CStr, task::Task, ARef, Opaque};
use core::{cell::UnsafeCell, marker::PhantomPinned, ops::Deref, pin::Pin};

/// Safely initialises a [`RwSemaphore`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! rwsemaphore_init {
    ($rwsem:expr, $name:literal) => {
        $crate::init_with_lockdep!($rwsem, $name)
    };
}

/// Exposes the kernel's [`struct rw_semaphore`].
///
/// It's a read/write mutex. That is, it allows multiple readers to acquire it concurrently, but
/// only one writer at a time. On contention, waiters sleep.
///
/// A [`RwSemaphore`] must first be initialised with a call to [`RwSemaphore::init_lock`] before it
/// can be used. The [`rwsemaphore_init`] macro is provided to automatically assign a new lock
/// class to an [`RwSemaphore`] instance.
///
/// Since it may block, [`RwSemaphore`] needs to be used with care in atomic contexts.
///
/// [`struct rw_semaphore`]: ../../../include/linux/rwsem.h
pub struct RwSemaphore<T: ?Sized> {
    /// The kernel `struct rw_semaphore` object.
    rwsem: Opaque<bindings::rw_semaphore>,

    /// An rwsem needs to be pinned because it contains a [`struct list_head`] that is
    /// self-referential, so it cannot be safely moved once it is initialised.
    _pin: PhantomPinned,

    /// The data protected by the rwsem.
    data: UnsafeCell<T>,
}

// SAFETY: `RwSemaphore` can be transferred across thread boundaries iff the data it protects can.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: ?Sized + Send> Send for RwSemaphore<T> {}

// SAFETY: `RwSemaphore` requires that the protected type be `Sync` for it to be `Sync` as well
// because the read mode allows multiple threads to access the protected data concurrently. It
// requires `Send` because the write lock allows a `&mut T` to be accessible from an arbitrary
// thread.
unsafe impl<T: ?Sized + Send + Sync> Sync for RwSemaphore<T> {}

impl<T> RwSemaphore<T> {
    /// Constructs a new rw semaphore.
    ///
    /// # Safety
    ///
    /// The caller must call [`RwSemaphore::init_lock`] before using the rw semaphore.
    pub unsafe fn new(t: T) -> Self {
        Self {
            rwsem: Opaque::uninit(),
            data: UnsafeCell::new(t),
            _pin: PhantomPinned,
        }
    }
}

impl<T: ?Sized> RwSemaphore<T> {
    /// Locks the rw semaphore in write (exclusive) mode and gives the caller access to the data
    /// protected by it. Only one thread at a time is allowed to access the protected data.
    pub fn write(&self) -> Guard<'_, Self> {
        let ctx = <Self as Lock>::lock_noguard(self);
        // SAFETY: The rw semaphore was just acquired in write mode.
        unsafe { Guard::new(self, ctx) }
    }

    /// Locks the rw semaphore in read (shared) mode and gives the caller access to the data
    /// protected by it. Only one thread at a time is allowed to access the protected data.
    pub fn read(&self) -> Guard<'_, Self, ReadLock> {
        let ctx = <Self as Lock<ReadLock>>::lock_noguard(self);
        // SAFETY: The rw semaphore was just acquired in read mode.
        unsafe { Guard::new(self, ctx) }
    }
}

impl<T> LockFactory for RwSemaphore<T> {
    type LockedType<U> = RwSemaphore<U>;

    unsafe fn new_lock<U>(data: U) -> RwSemaphore<U> {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { RwSemaphore::new(data) }
    }
}

impl<T> LockIniter for RwSemaphore<T> {
    fn init_lock(self: Pin<&mut Self>, name: &'static CStr, key: &'static LockClassKey) {
        unsafe { bindings::__init_rwsem(self.rwsem.get(), name.as_char_ptr(), key.get()) };
    }
}

// SAFETY: The underlying kernel `struct rw_semaphore` object ensures mutual exclusion because it's
// acquired in write mode.
unsafe impl<T: ?Sized> Lock for RwSemaphore<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `rwsem` points to valid memory. The current task won't be freed as long as the
        // lock is held (see the code below).
        unsafe { bindings::down_write(self.rwsem.get()) };

        // `down_write` sets the owner task as the `->owner` so that other tasks can inquiry the
        // running state for optimistic spinning. Therefore the ref-count of the owner needs to be
        // increased, otherwise if the task exists with the lock held, it may get freed and make
        // `->owner` a dangling pointer.
        let task: ARef<Task> = Task::current().into();
        core::mem::forget(task);

        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // Decreases the ref-count increased in `lock_noguard`.
        let task_ptr: core::ptr::NonNull<Task> = Task::current().deref().into();

        // SAFETY: The safety requirements of the function ensure that a `lock_noguard` has been
        // called, therefore the ref-count of the `task_ptr` has been increased.
        unsafe { ARef::from_raw(task_ptr) };

        // SAFETY: The safety requirements of the function ensure that the rw semaphore is owned by
        // the caller.
        unsafe { bindings::up_write(self.rwsem.get()) };
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

// SAFETY: The underlying kernel `struct rw_semaphore` object ensures that only shared references
// are accessible from other threads because it's acquired in read mode.
unsafe impl<T: ?Sized> Lock<ReadLock> for RwSemaphore<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `rwsem` points to valid memory.
        unsafe { bindings::down_read(self.rwsem.get()) };

        // `down_read` sets the owner task as the `->owner` so that other tasks can inquiry the
        // running state for optimistic spinning. Therefore the ref-count of the owner needs to be
        // increased, otherwise if the task exists with the lock held, it may get freed and make
        // `->owner` a dangling pointer.
        let task: ARef<Task> = Task::current().into();
        core::mem::forget(task);

        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // Decreases the ref-count increased in `lock_noguard`.
        let task_ptr: core::ptr::NonNull<Task> = Task::current().deref().into();

        // SAFETY: The safety requirements of the function ensure that a `lock_noguard` has been
        // called, therefore the ref-count of the `task_ptr` has been increased.
        unsafe { ARef::from_raw(task_ptr) };

        // SAFETY: The safety requirements of the function ensure that the rw semaphore is owned by
        // the caller.
        unsafe { bindings::up_read(self.rwsem.get()) };
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

/// A revocable rw semaphore.
///
/// That is, a read/write semaphore to which access can be revoked at runtime. It is a
/// specialisation of the more generic [`super::revocable::Revocable`].
///
/// # Examples
///
/// ```
/// # use kernel::sync::RevocableRwSemaphore;
/// # use kernel::revocable_init;
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn read_sum(v: &RevocableRwSemaphore<Example>) -> Option<u32> {
///     let guard = v.try_read()?;
///     Some(guard.a + guard.b)
/// }
///
/// fn add_two(v: &RevocableRwSemaphore<Example>) -> Option<u32> {
///     let mut guard = v.try_write()?;
///     guard.a += 2;
///     guard.b += 2;
///     Some(guard.a + guard.b)
/// }
///
/// // SAFETY: We call `revocable_init` immediately below.
/// let mut v = unsafe { RevocableRwSemaphore::new(Example { a: 10, b: 20 }) };
/// // SAFETY: We never move out of `v`.
/// let pinned = unsafe { Pin::new_unchecked(&mut v) };
/// revocable_init!(pinned, "example::v");
/// assert_eq!(read_sum(&v), Some(30));
/// assert_eq!(add_two(&v), Some(34));
/// v.revoke();
/// assert_eq!(read_sum(&v), None);
/// assert_eq!(add_two(&v), None);
/// ```
pub type RevocableRwSemaphore<T> = super::revocable::Revocable<RwSemaphore<()>, T>;

/// A guard for a revocable rw semaphore..
pub type RevocableRwSemaphoreGuard<'a, T, I = WriteLock> =
    super::revocable::RevocableGuard<'a, RwSemaphore<()>, T, I>;
