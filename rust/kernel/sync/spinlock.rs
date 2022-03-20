// SPDX-License-Identifier: GPL-2.0

//! A kernel spinlock.
//!
//! This module allows Rust code to use the kernel's [`struct spinlock`].
//!
//! See <https://www.kernel.org/doc/Documentation/locking/spinlocks.txt>.

use super::{mutex::EmptyGuardContext, CreatableLock, Guard, Lock, LockInfo, WriteLock};
use crate::{bindings, c_types, str::CStr, Opaque, True};
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
    pub const unsafe fn new(t: T) -> Self {
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
    pub fn lock(&self) -> Guard<'_, Self, WriteLock> {
        let ctx = <Self as Lock<WriteLock>>::lock_noguard(self);
        // SAFETY: The spinlock was just acquired.
        unsafe { Guard::new(self, ctx) }
    }

    /// Locks the spinlock and gives the caller access to the data protected by it. Additionally it
    /// disables interrupts (if they are enabled).
    ///
    /// When the lock in unlocked, the interrupt state (enabled/disabled) is restored.
    pub fn lock_irqdisable(&self) -> Guard<'_, Self, DisabledInterrupts> {
        let ctx = <Self as Lock<DisabledInterrupts>>::lock_noguard(self);
        // SAFETY: The spinlock was just acquired.
        unsafe { Guard::new(self, ctx) }
    }
}

impl<T> CreatableLock for SpinLock<T> {
    type CreateArgType = T;

    unsafe fn new_lock(data: Self::CreateArgType) -> Self {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { Self::new(data) }
    }

    unsafe fn init_lock(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key: *mut bindings::lock_class_key,
    ) {
        unsafe { bindings::__spin_lock_init(self.spin_lock.get(), name.as_char_ptr(), key) };
    }
}

/// A type state indicating that interrupts were disabled.
pub struct DisabledInterrupts;
impl LockInfo for DisabledInterrupts {
    type Writable = True;
}

// SAFETY: The underlying kernel `spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for SpinLock<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::spin_lock(self.spin_lock.get()) };
        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // SAFETY: The safety requirements of the function ensure that the spinlock is owned by
        // the caller.
        unsafe { bindings::spin_unlock(self.spin_lock.get()) }
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

// SAFETY: The underlying kernel `spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock<DisabledInterrupts> for SpinLock<T> {
    type Inner = T;
    type GuardContext = c_types::c_ulong;

    fn lock_noguard(&self) -> c_types::c_ulong {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::spin_lock_irqsave(self.spin_lock.get()) }
    }

    unsafe fn unlock(&self, ctx: &mut c_types::c_ulong) {
        // SAFETY: The safety requirements of the function ensure that the spinlock is owned by
        // the caller.
        unsafe { bindings::spin_unlock_irqrestore(self.spin_lock.get(), *ctx) }
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

/// Safely initialises an [`IrqDisableSpinLock`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! irqdisable_spinlock_init {
    ($spinlock:expr, $name:literal) => {
        $crate::init_with_lockdep!($spinlock, $name)
    };
}

/// A spinlock that always disables interrupts when locked.
///
/// This is similar to [`SpinLock`], except that the [`SpinLock::lock`] method always disables
/// interrupts, and the guard restores the state when dropped. That is,
/// [`IrqDisableSpinLock::lock`] behaves exactly like [`SpinLock::lock_irqdisable`].
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::sync::IrqDisableSpinLock;
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// // Function that acquires spinlock and disables interrupts while holding it.
/// fn lock_irqdisable_example(value: &IrqDisableSpinLock<Example>) {
///     let mut guard = value.lock();
///     guard.a = 10;
///     guard.b = 20;
/// }
///
/// // Initialises a spinlock and calls the example functions.
/// pub fn spinlock_example() {
///     // SAFETY: `spinlock_init` is called below.
///     let mut value = unsafe { IrqDisableSpinLock::new(Example { a: 1, b: 2 }) };
///     // SAFETY: We don't move `value`.
///     kernel::irqdisable_spinlock_init!(unsafe { Pin::new_unchecked(&mut value) }, "value");
///     lock_irqdisable_example(&value);
/// }
/// ```
pub struct IrqDisableSpinLock<T: ?Sized> {
    inner: SpinLock<T>,
}

// SAFETY: `IrqDisableSpinLock` can be transferred across thread boundaries iff the data it protects
// can.
unsafe impl<T: ?Sized + Send> Send for IrqDisableSpinLock<T> {}

// SAFETY: `IrqDisableSpinLock` serialises the interior mutability it provides, so it is `Sync` as
// long as the data it protects is `Send`.
unsafe impl<T: ?Sized + Send> Sync for IrqDisableSpinLock<T> {}

impl<T> IrqDisableSpinLock<T> {
    /// Constructs a new irq-disabling spinlock.
    ///
    /// # Safety
    ///
    /// The caller must call [`IrqDisableSpinLock::init_lock`] before using the spinlock.
    pub unsafe fn new(t: T) -> Self {
        // SAFETY: By the safety requirements of this function, The caller is required to call
        // `IrqDisableSpinLock::init_lock`, which itself calls `inner.init_lock`.
        Self {
            inner: unsafe { SpinLock::new(t) },
        }
    }
}

impl<T: ?Sized> IrqDisableSpinLock<T> {
    /// Locks the spinlock and gives the caller access to the data protected by it. Additionally it
    /// disables interrupts (if they are enabled).
    ///
    /// When the lock in unlocked, the interrupt state (enabled/disabled) is restored.
    pub fn lock(&self) -> GuardMut<'_, Self> {
        let ctx = self.lock_noguard();
        // SAFETY: The spinlock was just acquired.
        unsafe { GuardMut::new(self, ctx) }
    }
}

impl<T> CreatableLock for IrqDisableSpinLock<T> {
    unsafe fn new_lock(data: Self::Inner) -> Self {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { Self::new(data) }
    }

    unsafe fn init_lock(
        mut self: Pin<&mut Self>,
        name: &'static CStr,
        key: *mut bindings::lock_class_key,
    ) {
        // SAFETY: `self.inner` is pinned when `self` is.
        let inner = unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.inner) };
        // SAFETY: By the safety requirements of this function, `key` will remain valid.
        unsafe { inner.init_lock(name, key) };
    }
}

// SAFETY: The underlying kernel `spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for IrqDisableSpinLock<T> {
    type Inner = T;
    type GuardContext = <SpinLock<T> as Lock>::GuardContext;

    fn lock_noguard(&self) -> Self::GuardContext {
        Some(self.inner.internal_lock_irqsave())
    }

    unsafe fn unlock(&self, ctx: &mut Option<c_types::c_ulong>) {
        // SAFETY: The safety requirements of the function ensure that the spinlock is owned by the
        // caller.
        unsafe { self.inner.unlock(ctx) };
    }

    fn relock(&self, ctx: &mut Self::GuardContext) {
        self.inner.relock(ctx);
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        self.inner.locked_data()
    }
}
