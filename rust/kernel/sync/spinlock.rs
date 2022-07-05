// SPDX-License-Identifier: GPL-2.0

//! A kernel spinlock.
//!
//! This module allows Rust code to use the kernel's [`struct spinlock`].
//!
//! See <https://www.kernel.org/doc/Documentation/locking/spinlocks.txt>.

use super::{
    mutex::EmptyGuardContext, Guard, Lock, LockClassKey, LockFactory, LockInfo, LockIniter,
    WriteLock,
};
use crate::{bindings, str::CStr, Opaque, True};
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
/// // Initialises a spinlock.
/// // SAFETY: `spinlock_init` is called below.
/// let mut value = unsafe { SpinLock::new(Example { a: 1, b: 2 }) };
/// // SAFETY: We don't move `value`.
/// kernel::spinlock_init!(unsafe { Pin::new_unchecked(&mut value) }, "value");
///
/// // Calls the example functions.
/// assert_eq!(value.lock().a, 1);
/// lock_example(&value);
/// assert_eq!(value.lock().a, 10);
/// lock_irqdisable_example(&value);
/// assert_eq!(value.lock().a, 30);
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

impl<T> LockFactory for SpinLock<T> {
    type LockedType<U> = SpinLock<U>;

    unsafe fn new_lock<U>(data: U) -> SpinLock<U> {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { SpinLock::new(data) }
    }
}

impl<T> LockIniter for SpinLock<T> {
    fn init_lock(self: Pin<&mut Self>, name: &'static CStr, key: &'static LockClassKey) {
        unsafe { bindings::__spin_lock_init(self.spin_lock.get(), name.as_char_ptr(), key.get()) };
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
    type GuardContext = core::ffi::c_ulong;

    fn lock_noguard(&self) -> core::ffi::c_ulong {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::spin_lock_irqsave(self.spin_lock.get()) }
    }

    unsafe fn unlock(&self, ctx: &mut core::ffi::c_ulong) {
        // SAFETY: The safety requirements of the function ensure that the spinlock is owned by
        // the caller.
        unsafe { bindings::spin_unlock_irqrestore(self.spin_lock.get(), *ctx) }
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

/// Safely initialises a [`RawSpinLock`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! rawspinlock_init {
    ($spinlock:expr, $name:literal) => {
        $crate::init_with_lockdep!($spinlock, $name)
    };
}

/// Exposes the kernel's [`raw_spinlock_t`].
///
/// It is very similar to [`SpinLock`], except that it is guaranteed not to sleep even on RT
/// variants of the kernel.
///
/// # Examples
///
/// ```
/// # use kernel::sync::RawSpinLock;
/// # use core::pin::Pin;
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// // Function that acquires the raw spinlock without changing interrupt state.
/// fn lock_example(value: &RawSpinLock<Example>) {
///     let mut guard = value.lock();
///     guard.a = 10;
///     guard.b = 20;
/// }
///
/// // Function that acquires the raw spinlock and disables interrupts while holding it.
/// fn lock_irqdisable_example(value: &RawSpinLock<Example>) {
///     let mut guard = value.lock_irqdisable();
///     guard.a = 30;
///     guard.b = 40;
/// }
///
/// // Initialises a raw spinlock and calls the example functions.
/// fn spinlock_example() {
///     // SAFETY: `rawspinlock_init` is called below.
///     let mut value = unsafe { RawSpinLock::new(Example { a: 1, b: 2 }) };
///     // SAFETY: We don't move `value`.
///     kernel::rawspinlock_init!(unsafe { Pin::new_unchecked(&mut value) }, "value");
///     lock_example(&value);
///     lock_irqdisable_example(&value);
/// }
/// ```
///
/// [`raw_spinlock_t`]: ../../../include/linux/spinlock.h
pub struct RawSpinLock<T: ?Sized> {
    spin_lock: Opaque<bindings::raw_spinlock>,

    // Spinlocks are architecture-defined. So we conservatively require them to be pinned in case
    // some architecture uses self-references now or in the future.
    _pin: PhantomPinned,

    data: UnsafeCell<T>,
}

// SAFETY: `RawSpinLock` can be transferred across thread boundaries iff the data it protects can.
unsafe impl<T: ?Sized + Send> Send for RawSpinLock<T> {}

// SAFETY: `RawSpinLock` serialises the interior mutability it provides, so it is `Sync` as long as
// the data it protects is `Send`.
unsafe impl<T: ?Sized + Send> Sync for RawSpinLock<T> {}

impl<T> RawSpinLock<T> {
    /// Constructs a new raw spinlock.
    ///
    /// # Safety
    ///
    /// The caller must call [`RawSpinLock::init_lock`] before using the raw spinlock.
    pub const unsafe fn new(t: T) -> Self {
        Self {
            spin_lock: Opaque::uninit(),
            data: UnsafeCell::new(t),
            _pin: PhantomPinned,
        }
    }
}

impl<T: ?Sized> RawSpinLock<T> {
    /// Locks the raw spinlock and gives the caller access to the data protected by it. Only one
    /// thread at a time is allowed to access the protected data.
    pub fn lock(&self) -> Guard<'_, Self, WriteLock> {
        let ctx = <Self as Lock<WriteLock>>::lock_noguard(self);
        // SAFETY: The raw spinlock was just acquired.
        unsafe { Guard::new(self, ctx) }
    }

    /// Locks the raw spinlock and gives the caller access to the data protected by it.
    /// Additionally it disables interrupts (if they are enabled).
    ///
    /// When the lock in unlocked, the interrupt state (enabled/disabled) is restored.
    pub fn lock_irqdisable(&self) -> Guard<'_, Self, DisabledInterrupts> {
        let ctx = <Self as Lock<DisabledInterrupts>>::lock_noguard(self);
        // SAFETY: The raw spinlock was just acquired.
        unsafe { Guard::new(self, ctx) }
    }
}

impl<T> LockFactory for RawSpinLock<T> {
    type LockedType<U> = RawSpinLock<U>;

    unsafe fn new_lock<U>(data: U) -> RawSpinLock<U> {
        // SAFETY: The safety requirements of `new_lock` also require that `init_lock` be called.
        unsafe { RawSpinLock::new(data) }
    }
}

impl<T> LockIniter for RawSpinLock<T> {
    fn init_lock(self: Pin<&mut Self>, name: &'static CStr, key: &'static LockClassKey) {
        unsafe {
            bindings::_raw_spin_lock_init(self.spin_lock.get(), name.as_char_ptr(), key.get())
        };
    }
}

// SAFETY: The underlying kernel `raw_spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for RawSpinLock<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::raw_spin_lock(self.spin_lock.get()) };
        EmptyGuardContext
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // SAFETY: The safety requirements of the function ensure that the raw spinlock is owned by
        // the caller.
        unsafe { bindings::raw_spin_unlock(self.spin_lock.get()) };
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

// SAFETY: The underlying kernel `raw_spinlock_t` object ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock<DisabledInterrupts> for RawSpinLock<T> {
    type Inner = T;
    type GuardContext = core::ffi::c_ulong;

    fn lock_noguard(&self) -> core::ffi::c_ulong {
        // SAFETY: `spin_lock` points to valid memory.
        unsafe { bindings::raw_spin_lock_irqsave(self.spin_lock.get()) }
    }

    unsafe fn unlock(&self, ctx: &mut core::ffi::c_ulong) {
        // SAFETY: The safety requirements of the function ensure that the raw spinlock is owned by
        // the caller.
        unsafe { bindings::raw_spin_unlock_irqrestore(self.spin_lock.get(), *ctx) };
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}
