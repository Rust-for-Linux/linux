// SPDX-License-Identifier: GPL-2.0

//! A kernel spinlock.
//!
//! This module allows Rust code to use the kernel's [`struct spinlock`].
//!
//! See <https://www.kernel.org/doc/Documentation/locking/spinlocks.txt>.

use super::{
    mutex::EmptyGuardContext, Guard, Lock, LockClassKey, LockFactory, LockInfo, WriteLock,
};
use crate::{
    bindings,
    init::{self, PinInit},
    macros::pin_data,
    pin_init,
    str::CStr,
    Opaque, True,
};
use core::{cell::UnsafeCell, marker::PhantomPinned};

/// Safely initialises a [`SpinLock`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! new_spinlock {
    ($value:expr, $name:literal) => {
        $crate::new_with_lockdep!($crate::sync::SpinLock<_>, $name, $value)
    };
}

/// Exposes the kernel's [`spinlock_t`]. When multiple CPUs attempt to lock the same spinlock, only
/// one at a time is allowed to progress, the others will block (spinning) until the spinlock is
/// unlocked, at which point another CPU will be allowed to make progress.
///
/// A [`SpinLock`] is created using the [initialization API][init]. You can either call the `new`
/// function or use the [`new_spinlock!`] macro which automatically creates the [`LockClassKey`] for you.
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
/// # use kernel::{new_spinlock, stack_init, sync::SpinLock};
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
/// stack_init!(let value = new_spinlock!(Example { a: 1, b: 2 }, "value"));
/// let value = value.unwrap();
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
/// [init]: ../init/index.html
#[pin_data]
pub struct SpinLock<T: ?Sized> {
    #[pin]
    spin_lock: Opaque<bindings::spinlock>,

    /// Spinlocks are architecture-defined. So we conservatively require them to be pinned in case
    /// some architecture uses self-references now or in the future.
    #[pin]
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
    #[allow(clippy::new_ret_no_self)]
    pub const fn new(
        data: T,
        name: &'static CStr,
        key: &'static LockClassKey,
    ) -> impl PinInit<Self> {
        Init { data, name, key }
    }
}

#[doc(hidden)]
pub struct Init<T> {
    name: &'static CStr,
    key: &'static LockClassKey,
    data: T,
}

unsafe impl<T> PinInit<SpinLock<T>> for Init<T> {
    unsafe fn __pinned_init(
        self,
        slot: *mut SpinLock<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        let init = pin_init!(SpinLock<T> {
            // SAFETY: __spin_lock_init is an initializer function and name and key are valid
            // parameters.
            spin_lock: unsafe {
                init::common::ffi_init2(
                    bindings::__spin_lock_init,
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
    type Error = core::convert::Infallible;
    type Init<U> = Init<U>;

    fn new_lock<U>(data: U, name: &'static CStr, key: &'static LockClassKey) -> Self::Init<U> {
        Init { data, name, key }
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
macro_rules! new_rawspinlock {
    ($value:expr, $name:literal) => {
        $crate::new_with_lockdep!($crate::sync::RawSpinLock<_>, $name, $value)
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
/// # use kernel::{sync::RawSpinLock, stack_init, new_rawspinlock};
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
///     stack_init!(let value = new_rawspinlock!(Example { a: 1, b: 2 }, "value"));
///     let value = value.unwrap();
///     lock_example(&value);
///     lock_irqdisable_example(&value);
/// }
/// ```
///
/// [`raw_spinlock_t`]: ../../../include/linux/spinlock.h
#[pin_data]
pub struct RawSpinLock<T: ?Sized> {
    #[pin]
    spin_lock: Opaque<bindings::raw_spinlock>,

    // Spinlocks are architecture-defined. So we conservatively require them to be pinned in case
    // some architecture uses self-references now or in the future.
    #[pin]
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
    #[allow(clippy::new_ret_no_self)]
    pub const fn new(
        data: T,
        name: &'static CStr,
        key: &'static LockClassKey,
    ) -> impl PinInit<Self> {
        RInit { data, name, key }
    }
}

#[doc(hidden)]
pub struct RInit<T> {
    name: &'static CStr,
    key: &'static LockClassKey,
    data: T,
}

unsafe impl<T> PinInit<RawSpinLock<T>> for RInit<T> {
    unsafe fn __pinned_init(
        self,
        slot: *mut RawSpinLock<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        let init = pin_init!(RawSpinLock<T> {
            // SAFETY: _raw_spin_lock_init is an initializer function and name and key are valid
            // parameters.
            spin_lock: unsafe {
                init::common::ffi_init2(
                    bindings::_raw_spin_lock_init,
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
    type Error = core::convert::Infallible;
    type Init<U> = RInit<U>;

    fn new_lock<U>(data: U, name: &'static CStr, key: &'static LockClassKey) -> Self::Init<U> {
        RInit { data, name, key }
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
