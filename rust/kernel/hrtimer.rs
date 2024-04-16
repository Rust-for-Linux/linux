// SPDX-License-Identifier: GPL-2.0

//! Intrusive high resolution timers.
//!
//! Allows scheduling timer callbacks without doing allocations at the time of
//! scheduling. For now, only one timer per type is allowed.
//!
//! # Example
//!
//! ```rust
//! use kernel::{
//!     sync::Arc, hrtimer::{RawTimer, Timer, TimerCallback},
//!     impl_has_timer, prelude::*, stack_pin_init
//! };
//! use core::sync::atomic::AtomicBool;
//! use core::sync::atomic::Ordering;
//!
//! #[pin_data]
//! struct IntrusiveTimer {
//!     #[pin]
//!     timer: Timer<Self>,
//!     flag: AtomicBool,
//! }
//!
//! impl IntrusiveTimer {
//!     fn new() -> impl PinInit<Self> {
//!         pin_init!(Self {
//!             timer <- Timer::new(),
//!             flag: AtomicBool::new(false),
//!         })
//!     }
//! }
//!
//! impl TimerCallback for IntrusiveTimer {
//!     type Receiver = Arc<IntrusiveTimer>;
//!
//!     fn run(this: Self::Receiver) {
//!         pr_info!("Timer called\n");
//!         this.flag.store(true, Ordering::Relaxed);
//!     }
//! }
//!
//! impl_has_timer! {
//!     impl HasTimer<Self> for IntrusiveTimer { self.timer }
//! }
//!
//! let has_timer = Arc::pin_init(IntrusiveTimer::new())?;
//! has_timer.clone().schedule(200_000_000);
//! while !has_timer.flag.load(Ordering::Relaxed) { core::hint::spin_loop() }
//!
//! pr_info!("Flag raised\n");
//!
//! # Ok::<(), kernel::error::Error>(())
//! ```
//!
//! C header: [`include/linux/hrtimer.h`](srctree/include/linux/hrtimer.h)

use core::{marker::PhantomData, pin::Pin};

use crate::{init::PinInit, prelude::*, sync::Arc, types::Opaque};

/// A timer backed by a C `struct hrtimer`
///
/// # Invariants
///
/// * `self.timer` is initialized by `bindings::hrtimer_init`.
#[repr(transparent)]
#[pin_data(PinnedDrop)]
pub struct Timer<T> {
    #[pin]
    timer: Opaque<bindings::hrtimer>,
    _t: PhantomData<T>,
}

// SAFETY: A `Timer` can be moved to other threads and used from there.
unsafe impl<T> Send for Timer<T> {}

// SAFETY: Timer operations are locked on C side, so it is safe to operate on a
// timer from multiple threads
unsafe impl<T> Sync for Timer<T> {}

impl<T: TimerCallback> Timer<T> {
    /// Return an initializer for a new timer instance.
    pub fn new() -> impl PinInit<Self> {
        crate::pin_init!( Self {
            timer <- Opaque::ffi_init(move |place: *mut bindings::hrtimer| {
                // SAFETY: By design of `pin_init!`, `place` is a pointer live
                // allocation. hrtimer_init will initialize `place` and does not
                // require `place` to be initialized prior to the call.
                unsafe {
                    bindings::hrtimer_init(
                        place,
                        bindings::CLOCK_MONOTONIC as i32,
                        bindings::hrtimer_mode_HRTIMER_MODE_REL,
                    );
                }

                // SAFETY: `place` is pointing to a live allocation, so the deref
                // is safe. The `function` field might not be initialized, but
                // `addr_of_mut` does not create a reference to the field.
                let function: *mut Option<_> = unsafe { core::ptr::addr_of_mut!((*place).function) };

                // SAFETY: `function` points to a valid allocation.
                unsafe { core::ptr::write(function, Some(T::Receiver::run)) };
            }),
            _t: PhantomData,
        })
    }
}

#[pinned_drop]
impl<T> PinnedDrop for Timer<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: By struct invariant `self.timer` was initialized by
        // `hrtimer_init` so by C API contract it is safe to call
        // `hrtimer_cancel`.
        unsafe {
            bindings::hrtimer_cancel(self.timer.get());
        }
    }
}

/// Implemented by pointer types to structs that embed a [`Timer`]. This trait
/// facilitates queueing the timer through the pointer that implements the
/// trait.
///
/// Typical implementers would be [`Box<T>`], [`Arc<T>`], [`ARef<T>`] where `T`
/// has a field of type `Timer`.
///
/// Target must be [`Sync`] because timer callbacks happen in another thread of
/// execution.
///
/// [`Box<T>`]: Box
/// [`Arc<T>`]: Arc
/// [`ARef<T>`]: crate::types::ARef
pub trait RawTimer: Sync {
    /// Schedule the timer after `expires` time units
    fn schedule(self, expires: u64);
}

/// Implemented by structs that contain timer nodes.
///
/// Clients of the timer API would usually safely implement this trait by using
/// the [`impl_has_timer`] macro.
///
/// # Safety
///
/// Implementers of this trait must ensure that the implementer has a [`Timer`]
/// field at the offset specified by `OFFSET` and that all trait methods are
/// implemented according to their documentation.
///
/// [`impl_has_timer`]: crate::impl_has_timer
pub unsafe trait HasTimer<T> {
    /// Offset of the [`Timer`] field within `Self`
    const OFFSET: usize;

    /// Return a pointer to the [`Timer`] within `Self`.
    ///
    /// # Safety
    ///
    /// `ptr` must point to a valid struct of type `Self`.
    unsafe fn raw_get_timer(ptr: *const Self) -> *const Timer<T> {
        // SAFETY: By the safety requirement of this trait, the trait
        // implementor will have a `Timer` field at the specified offset.
        unsafe { ptr.cast::<u8>().add(Self::OFFSET).cast::<Timer<T>>() }
    }

    /// Return a pointer to the struct that is embedding the [`Timer`] pointed
    /// to by `ptr`.
    ///
    /// # Safety
    ///
    /// `ptr` must point to a [`Timer<T>`] field in a struct of type `Self`.
    unsafe fn timer_container_of(ptr: *mut Timer<T>) -> *mut Self
    where
        Self: Sized,
    {
        // SAFETY: By the safety requirement of this trait, the trait
        // implementor will have a `Timer` field at the specified offset.
        unsafe { ptr.cast::<u8>().sub(Self::OFFSET).cast::<Self>() }
    }
}

/// Implemented by pointer types that can be the target of a C timer callback.
pub trait RawTimerCallback: RawTimer {
    /// Callback to be called from C.
    ///
    /// # Safety
    ///
    /// Only to be called by C code in `hrtimer`subsystem.
    unsafe extern "C" fn run(ptr: *mut bindings::hrtimer) -> bindings::hrtimer_restart;
}

/// Implemented by pointers to structs that can the target of a timer callback
pub trait TimerCallback {
    /// Type of `this` argument for `run()`.
    type Receiver: RawTimerCallback;

    /// Called by the timer logic when the timer fires
    fn run(this: Self::Receiver);
}

impl<T> RawTimer for Arc<T>
where
    T: Send + Sync,
    T: HasTimer<T>,
{
    fn schedule(self, expires: u64) {
        let self_ptr = Arc::into_raw(self);

        // SAFETY: `self_ptr` is a valid pointer to a `T`
        let timer_ptr = unsafe { T::raw_get_timer(self_ptr) };

        // `Timer` is `repr(transparent)`
        let c_timer_ptr = timer_ptr.cast::<bindings::hrtimer>();

        // Schedule the timer - if it is already scheduled it is removed and
        // inserted

        // SAFETY: c_timer_ptr points to a valid hrtimer instance that was
        // initialized by `hrtimer_init`
        unsafe {
            bindings::hrtimer_start_range_ns(
                c_timer_ptr.cast_mut(),
                expires as i64,
                0,
                bindings::hrtimer_mode_HRTIMER_MODE_REL,
            );
        }
    }
}

impl<T> kernel::hrtimer::RawTimerCallback for Arc<T>
where
    T: Send + Sync,
    T: HasTimer<T>,
    T: TimerCallback<Receiver = Self>,
{
    unsafe extern "C" fn run(ptr: *mut bindings::hrtimer) -> bindings::hrtimer_restart {
        // `Timer` is `repr(transparent)`
        let timer_ptr = ptr.cast::<kernel::hrtimer::Timer<T>>();

        // SAFETY: By C API contract `ptr` is the pointer we passed when
        // enqueing the timer, so it is a `Timer<T>` embedded in a `T`
        let data_ptr = unsafe { T::timer_container_of(timer_ptr) };

        // SAFETY: This `Arc` comes from a call to `Arc::into_raw()`
        let receiver = unsafe { Arc::from_raw(data_ptr) };

        T::run(receiver);

        bindings::hrtimer_restart_HRTIMER_NORESTART
    }
}

/// Use to implement the [`HasTimer<T>`] trait.
///
/// See [`module`] documentation for an example.
///
/// [`module`]: crate::hrtimer
#[macro_export]
macro_rules! impl_has_timer {
    ($(impl$(<$($implarg:ident),*>)?
       HasTimer<$timer_type:ty $(, $id:tt)?>
       for $self:ident $(<$($selfarg:ident),*>)?
       { self.$field:ident }
    )*) => {$(
        // SAFETY: This implementation of `raw_get_timer` only compiles if the
        // field has the right type.
        unsafe impl$(<$($implarg),*>)? $crate::hrtimer::HasTimer<$timer_type> for $self $(<$($selfarg),*>)? {
            const OFFSET: usize = ::core::mem::offset_of!(Self, $field) as usize;

            #[inline]
            unsafe fn raw_get_timer(ptr: *const Self) -> *const $crate::hrtimer::Timer<$timer_type $(, $id)?> {
                // SAFETY: The caller promises that the pointer is not dangling.
                unsafe {
                    ::core::ptr::addr_of!((*ptr).$field)
                }
            }

        }
    )*};
}
