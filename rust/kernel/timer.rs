// SPDX-License-Identifier: GPL-2.0
//
//! Timer abstraction.
//!
//! C header: [`include/linux/timer.h`](../../../../include/linux/timer.h)

use crate::bindings;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::addr_of_mut;
use core::time::Duration;

/// Trait which must be implemented by driver-specific timer objects.
pub trait TimerOps: Sized {
    /// Type of the Inner data inside the Timer
    type Inner;

    /// Timer callback
    fn timer_callback(timer: &UniqueTimer<Self, Self::Inner>);
}

unsafe extern "C" fn timer_callback<T: TimerOps<Inner = D>, D: Sized>(
    timer: *mut bindings::timer_list,
) {
    let timer = crate::container_of!(timer, UniqueTimer<T, D>, timer) as *mut UniqueTimer<T, D>;

    // SAFETY: The caller is responsible for passing a valid timer_list subtype
    T::timer_callback(unsafe { &mut *timer });
}

/// A generic Timer Object
///
/// This object should be instantiated by the end user, as it holds
/// a unique reference to the struct UniqueTimer. The UniqueTimer
/// methods can be used through it.
pub struct Timer<T: TimerOps<Inner = D>, D>(*mut UniqueTimer<T, D>);

impl<T: TimerOps<Inner = D>, D> Timer<T, D> {
    /// Create a timer for its first use
    pub fn setup(inner: D) -> Self {
        let t = unsafe {
            bindings::krealloc(
                core::ptr::null_mut(),
                core::mem::size_of::<UniqueTimer<T, D>>(),
                bindings::GFP_KERNEL | bindings::__GFP_ZERO,
            ) as *mut UniqueTimer<T, D>
        };

        // SAFETY: The pointer is valid, so pointers to members are too.
        // After this, all fields are initialized.
        unsafe {
            addr_of_mut!((*t).inner).write(inner);
            bindings::timer_setup(addr_of_mut!((*t).timer), Some(timer_callback::<T, D>), 0)
        };

        Self(t)
    }
}

impl<T: TimerOps<Inner = D>, D> Drop for Timer<T, D> {
    fn drop(&mut self) {
        // SAFETY: inner is never used after this
        unsafe {
            core::ptr::drop_in_place(&mut (*self.0).inner);
        }

        // SAFETY: All of our timers are allocated using kmalloc, so this is safe.
        unsafe { bindings::del_timer_sync(self.raw()) };
    }
}

impl<T: TimerOps<Inner = D>, D> Deref for Timer<T, D> {
    type Target = UniqueTimer<T, D>;

    fn deref(&self) -> &UniqueTimer<T, D> {
        unsafe { &*self.0 }
    }
}

impl<T: TimerOps<Inner = D>, D> DerefMut for Timer<T, D> {
    fn deref_mut(&mut self) -> &mut UniqueTimer<T, D> {
        unsafe { &mut *self.0 }
    }
}

/// A driver-specific Timer Object
///
/// # Invariants
/// timer is a valid pointer to a struct timer_list and we own a reference to it.
#[repr(C)]
pub struct UniqueTimer<T: TimerOps<Inner = D>, D> {
    timer: bindings::timer_list,
    inner: D,
    _p: PhantomData<T>,
}

impl<T: TimerOps<Inner = D>, D> UniqueTimer<T, D> {
    /// Modify a timer's timeout
    pub fn modify(&self, duration: Duration) {
        let duration =
            unsafe { bindings::msecs_to_jiffies(duration.as_millis().try_into().unwrap()) };

        // SAFETY: As defined in the invariants, timer is a valid pointer.
        unsafe { bindings::mod_timer(self.raw(), bindings::jiffies + duration) };
    }

    /// Returns the inner value of timer
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Returns the raw `struct timer_list` pointer.
    pub fn raw(&self) -> *mut bindings::timer_list {
        &self.timer as *const _ as *mut _
    }
}
