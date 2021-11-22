// SPDX-License-Identifier: GPL-2.0

//! A kernel spinlock.
//!
//! This module allows Rust code to use the kernel's [`struct spinlock`].
//!
//! See <https://www.kernel.org/doc/Documentation/locking/spinlocks.txt>.

use super::{CreatableLock, GuardMut, Lock};
use crate::{bindings, c_types, str::CStr, Opaque};
use core::{cell::UnsafeCell, marker::PhantomPinned, pin::Pin};

/// Safely initialises a [`SpinLock`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! spinlock_init {
    ($spinlock:expr, $name:literal) => {
        $crate::init_with_lockdep!($spinlock, $name)
    };
}

/// Exposes the kernel's [`spinlock_t`]. When multiple CPUs attempt to lock the same spinlock, only
/// one at a time is allowed to progress, the others will block (spinning) until the spinlock is
/// unlocked, at which point another CPU will be allowed to make progress.
///
/// A [`SpinLock`] must first be initialised with a call to [`SpinLock::init_lock`] before it can be
/// used. The [`spinlock_init`] macro is provided to automatically assign a new lock class to a
/// spinlock instance.
///
/// There are two ways to acquire the lock:
///  - [`SpinLock::lock`], which doesn't manage interrupt state, so it should be used in only two
///    cases: (a) when the caller knows that interrupts are disabled, or (b) when callers never use
///    it in atomic context (e.g., interrupt handlers), in which case it is ok for interrupts to be
///    enabled.
///  - [`SpinLock::lock_irqdisable`], which disables interrupts if they are enabled before
///    acquiring the lock. When the lock is released, the interrupt state is automatically returned
///    to its value before [`SpinLock::lock_irqdisable`] was called.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::sync::SpinLock;
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// // Function that acquires spinlock without changing interrupt state.
/// fn lock_example(value: &SpinLock<Example>) {
///     let mut guard = value.lock();
///     guard.a = 10;
///     guard.b = 20;
/// }
///
/// // Function that acquires spinlock and disables interrupts while holding it.
/// fn lock_irqdisable_example(value: &SpinLock<Example>) {
///     let mut guard = value.lock_irqdisable();
///     guard.a = 30;
///     guard.b = 40;
/// }
///
/// // Initialises a spinlock and calls the example functions.
/// pub fn spinlock_example() {
///     // SAFETY: `spinlock_init` is called below.
///     let mut value = unsafe { SpinLock::new(Example { a: 1, b: 2 }) };
///     // SAFETY: We don't move `value`.
///     kernel::spinlock_init!(unsafe { Pin::new_unchecked(&mut value) }, "value");
///     lock_example(&value);
///     lock_irqdisable_example(&value);
/// }
/// ```
///
/// [`spinlock_t`]: ../../../include/linux/spinlock.h
pub struct SpinLock<T: ?Sized> {
    spin_lock: Opaque<bindings::spinlock>,

    /// Spinlocks are architecture-defined. So we conservatively require them to be pinned in case
    /// some architecture uses self-references now or in the future.
    _pin: PhantomPinned,

    data: UnsafeCell<T>,
}

// SAFETY: `SpinLock` can be transferred across thread boundaries iff the data it protects can.
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

// SAFETY: `SpinLock` serialises the interior mutability it provides, so it is `Sync` as long as the
// data it protects is `Send`.
unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// Constructs a new spinlock.
    ///
    /// # Safety
    ///
    /// The caller must call [`SpinLock::init_lock`] before using the spinlock.
    pub unsafe fn new(t: T) -> Self {
        Self {
            spin_lock: Opaque::uninit(),
            data: UnsafeCell::new(t),
            _pin: PhantomPinned,
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    /// Locks the spinlock and gives the caller access to the data protected by it. Only one thread
    /// at a time is allowed to access the protected data.
    pub fn lock(&self) -> GuardMut<'_, Self> {
        let ctx = self.lock_noguard();
        // SAFETY: The spinlock was just acquired.
        unsafe { GuardMut::new(self, ctx) }
    }

    /// Locks the spinlock and gives the caller access to the data protected by it. Additionally it
    /// disables interrupts (if they are enabled).
    ///
    /// When the lock in unlocked, the interrupt state (enabled/disabled) is restored.
    pub fn lock_irqdisable(&self) -> GuardMut<'_, Self> {
        let ctx = self.internal_lock_irqsave();
        // SAFETY: The spinlock was just acquired.
        unsafe { GuardMut::new(self, Some(ctx)) }
    }

    fn internal_lock_irqsave(&self) -> c_types::c_ulong {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::spin_lock_irqsave(self.spin_lock.get()) }
    }
}

impl<T> CreatableLock for SpinLock<T> {
    unsafe fn new_lock(data: Self::Inner) -> Self {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { Self::new(data) }
    }

    unsafe fn init_lock(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key: *mut bindings::lock_class_key,
    ) {
        // SAFETY:
        // `spin_lock` is a valid uninitialised C `spin_lock`.
        // `name` is a valid `NUL`-Terminated `CStr`.
        // `key` The safety requirements of `init_lock` require that it is
        //  valid and will remain valid for the lifetime for the lock.
        unsafe { bindings::__spin_lock_init(self.spin_lock.get(), name.as_char_ptr(), key) };
    }
}

// SAFETY: The underlying kernel `spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for SpinLock<T> {
    type Inner = T;
    type GuardContext = Option<c_types::c_ulong>;

    fn lock_noguard(&self) -> Option<c_types::c_ulong> {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::spin_lock(self.spin_lock.get()) };
        None
    }

    unsafe fn unlock(&self, ctx: &mut Option<c_types::c_ulong>) {
        match ctx {
            // SAFETY: The safety requirements of the function ensure that the spinlock is owned by
            // the caller.
            Some(v) => unsafe { bindings::spin_unlock_irqrestore(self.spin_lock.get(), *v) },
            // SAFETY: The safety requirements of the function ensure that the spinlock is owned by
            // the caller.
            None => unsafe { bindings::spin_unlock(self.spin_lock.get()) },
        }
    }

    fn relock(&self, ctx: &mut Self::GuardContext) {
        match ctx {
            Some(v) => *v = self.internal_lock_irqsave(),
            None => *ctx = self.lock_noguard(),
        }
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}
