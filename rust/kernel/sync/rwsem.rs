// SPDX-License-Identifier: GPL-2.0

//! A kernel read/write mutex.
//!
//! This module allows Rust code to use the kernel's [`struct rw_semaphore`].
//!
//! C header: [`include/linux/rwsem.h`](../../../../include/linux/rwsem.h)

use super::{
    mutex::EmptyGuardContext, Guard, Lock, LockClassKey, LockFactory, ReadLock, WriteLock,
};
use crate::{
    bindings,
    init::{self, PinInit},
    macros::pin_data,
    pin_init,
    str::CStr,
    Opaque,
};
use core::{cell::UnsafeCell, marker::PhantomPinned};

/// Safely initialises a [`RwSemaphore`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! new_rwsemaphore {
    ($value:expr, $name:literal) => {
        $crate::new_with_lockdep!($crate::sync::RwSemaphore<_>, $name, $value)
    };
}

/// Exposes the kernel's [`struct rw_semaphore`].
///
/// It's a read/write mutex. That is, it allows multiple readers to acquire it concurrently, but
/// only one writer at a time. On contention, waiters sleep.
///
/// A [`RwSemaphore`] is created using the [initialization API][init]. You can either call the `new`
/// function or use the [`new_rwsemaphore!`] macro which automatically creates the [`LockClassKey`] for you.
///
/// Since it may block, [`RwSemaphore`] needs to be used with care in atomic contexts.
///
/// [`struct rw_semaphore`]: ../../../include/linux/rwsem.h
/// [init]: ../init/index.html
#[pin_data]
pub struct RwSemaphore<T: ?Sized> {
    /// The kernel `struct rw_semaphore` object.
    #[pin]
    rwsem: Opaque<bindings::rw_semaphore>,

    /// An rwsem needs to be pinned because it contains a [`struct list_head`] that is
    /// self-referential, so it cannot be safely moved once it is initialised.
    #[pin]
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
    #[allow(clippy::new_ret_no_self)]
    pub fn new(data: T, name: &'static CStr, key: &'static LockClassKey) -> impl PinInit<Self> {
        Init { data, name, key }
    }
}

#[doc(hidden)]
pub struct Init<T> {
    name: &'static CStr,
    key: &'static LockClassKey,
    data: T,
}

unsafe impl<T> PinInit<RwSemaphore<T>> for Init<T> {
    unsafe fn __pinned_init(
        self,
        slot: *mut RwSemaphore<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        let init = pin_init!(RwSemaphore<T> {
            // SAFETY: __init_rwsem is an initializer function and name and key are valid
            // parameters.
            rwsem: unsafe {
                init::common::ffi_init2(
                    bindings::__init_rwsem,
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
    type Error = core::convert::Infallible;
    type Init<U> = Init<U>;

    fn new_lock<U>(data: U, name: &'static CStr, key: &'static LockClassKey) -> Self::Init<U> {
        Init { data, name, key }
    }
}

// SAFETY: The underlying kernel `struct rw_semaphore` object ensures mutual exclusion because it's
// acquired in write mode.
unsafe impl<T: ?Sized> Lock for RwSemaphore<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `rwsem` points to valid memory.
        unsafe { bindings::down_write(self.rwsem.get()) };
        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
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
        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
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
/// # use kernel::{new_revocable, stack_init};
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
/// stack_init!(let v: RevocableRwSemaphore<_> = new_revocable!(Example { a: 10, b: 20 }, "example::v"));
/// let v = v.unwrap();
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
