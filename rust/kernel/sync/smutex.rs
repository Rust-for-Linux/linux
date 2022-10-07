// SPDX-License-Identifier: GPL-2.0

//! A simple mutex implementation.
//!
//! Differently from [`super::Mutex`], this implementation does not require pinning, so the
//! ergonomics are much improved, though the implementation is not as feature-rich as the C-based
//! one. The main advantage is that it doesn't impose unsafe blocks on callers.
//!
//! The mutex is made up of 2 words in addition to the data it protects. The first one is accessed
//! concurrently by threads trying to acquire and release the mutex, it contains a "stack" of
//! waiters and a "locked" bit; the second one is only accessible by the thread holding the mutex,
//! it contains a queue of waiters. Waiters are moved from the stack to the queue when the mutex is
//! next unlocked while the stack is non-empty and the queue is empty. A single waiter is popped
//! from the wait queue when the owner of the mutex unlocks it.
//!
//! The initial state of the mutex is `<locked=0, stack=[], queue=[]>`, meaning that it isn't
//! locked and both the waiter stack and queue are empty.
//!
//! A lock operation transitions the mutex to state `<locked=1, stack=[], queue=[]>`.
//!
//! An unlock operation transitions the mutex back to the initial state, however, an attempt to
//! lock the mutex while it's already locked results in a waiter being created (on the stack) and
//! pushed onto the stack, so the state is `<locked=1, stack=[W1], queue=[]>`.
//!
//! Another thread trying to lock the mutex results in another waiter being pushed onto the stack,
//! so the state becomes `<locked=1, stack=[W2, W1], queue=[]>`.
//!
//! In such states (queue is empty but stack is non-empty), the unlock operation is performed in
//! three steps:
//! 1. The stack is popped (but the mutex remains locked), so the state is:
//!    `<locked=1, stack=[], queue=[]>`
//! 2. The stack is turned into a queue by reversing it, so the state is:
//!    `<locked=1, stack=[], queue=[W1, W2]>
//! 3. Finally, the lock is released, and the first waiter is awakened, so the state is:
//!    `<locked=0, stack=[], queue=[W2]>`
//!
//! The mutex remains accessible to any threads attempting to lock it in any of the intermediate
//! states above. For example, while it is locked, other threads may add waiters to the stack
//! (which is ok because we want to release the ones on the queue first); another example is that
//! another thread may acquire the mutex before waiter W1 in the example above, this makes the
//! mutex unfair but this is desirable because the thread is running already and may in fact
//! release the lock before W1 manages to get scheduled -- it also mitigates the lock convoy
//! problem when the releasing thread wants to immediately acquire the lock again: it will be
//! allowed to do so (as long as W1 doesn't get to it first).
//!
//! When the waiter queue is non-empty, unlocking the mutex always results in the first waiter being
//! popped form the queue and awakened.

use super::{mutex::EmptyGuardContext, Guard, Lock, LockClassKey, LockFactory};
use crate::{
    bindings,
    init::{Init, PinInit},
    str::CStr,
    Opaque,
};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// The value that is OR'd into the [`Mutex::waiter_stack`] when the mutex is locked.
const LOCKED: usize = 1;

/// A simple mutex.
///
/// This is mutual-exclusion primitive. It guarantees that only one thread at a time may access the
/// data it protects. When multiple threads attempt to lock the same mutex, only one at a time is
/// allowed to progress, the others will block (sleep) until the mutex is unlocked, at which point
/// another thread will be allowed to wake up and make progress.
///
/// # Examples
///
/// ```
/// # use kernel::{Result, sync::Arc, sync::smutex::Mutex};
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// static EXAMPLE: Mutex<Example> = Mutex::new(Example { a: 10, b: 20 });
///
/// fn inc_a(example: &Mutex<Example>) {
///     let mut guard = example.lock();
///     guard.a += 1;
/// }
///
/// fn sum(example: &Mutex<Example>) -> u32 {
///     let guard = example.lock();
///     guard.a + guard.b
/// }
///
/// fn try_new(a: u32, b: u32) -> Result<Arc<Mutex<Example>>> {
///     Arc::try_new(Mutex::new(Example { a, b }))
/// }
///
/// assert_eq!(EXAMPLE.lock().a, 10);
/// assert_eq!(sum(&EXAMPLE), 30);
///
/// inc_a(&EXAMPLE);
///
/// assert_eq!(EXAMPLE.lock().a, 11);
/// assert_eq!(sum(&EXAMPLE), 31);
///
/// # try_new(42, 43);
/// ```
pub struct Mutex<T: ?Sized> {
    /// A stack of waiters.
    ///
    /// It is accessed atomically by threads lock/unlocking the mutex. Additionally, the
    /// least-significant bit is used to indicate whether the mutex is locked or not.
    waiter_stack: AtomicUsize,

    /// A queue of waiters.
    ///
    /// This is only accessible to the holder of the mutex. When the owner of the mutex is
    /// unlocking it, it will move waiters from the stack to the queue when the queue is empty and
    /// the stack non-empty.
    waiter_queue: UnsafeCell<*mut Waiter>,

    /// The data protected by the mutex.
    data: UnsafeCell<T>,
}

// SAFETY: `Mutex` can be transferred across thread boundaries iff the data it protects can.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}

// SAFETY: `Mutex` serialises the interior mutability it provides, so it is `Sync` as long as the
// data it protects is `Send`.
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Creates a new instance of the mutex.
    pub const fn new(data: T) -> Self {
        Self {
            waiter_stack: AtomicUsize::new(0),
            waiter_queue: UnsafeCell::new(core::ptr::null_mut()),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Locks the mutex and gives the caller access to the data protected by it. Only one thread at
    /// a time is allowed to access the protected data.
    pub fn lock(&self) -> Guard<'_, Self> {
        let ctx = self.lock_noguard();
        // SAFETY: The mutex was just acquired.
        unsafe { Guard::new(self, ctx) }
    }
}

#[doc(hidden)]
pub struct MutexInit<T> {
    data: T,
}

unsafe impl<T> PinInit<Mutex<T>> for MutexInit<T> {
    unsafe fn __pinned_init(
        self,
        slot: *mut Mutex<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        unsafe { self.__init(slot) }
    }
}

unsafe impl<T> Init<Mutex<T>> for MutexInit<T> {
    unsafe fn __init(
        self,
        slot: *mut Mutex<T>,
    ) -> core::result::Result<(), core::convert::Infallible> {
        // SAFETY: pointer is valid
        unsafe { slot.write(Mutex::new(self.data)) };
        Ok(())
    }
}

impl<T> LockFactory for Mutex<T> {
    type LockedType<U> = Mutex<U>;
    type Error = core::convert::Infallible;
    type Init<U> = MutexInit<U>;

    fn new_lock<U>(data: U, _: &'static CStr, _: &'static LockClassKey) -> Self::Init<U> {
        MutexInit { data }
    }
}

// SAFETY: The mutex implementation ensures mutual exclusion.
unsafe impl<T: ?Sized> Lock for Mutex<T> {
    type Inner = T;
    type GuardContext = EmptyGuardContext;

    fn lock_noguard(&self) -> EmptyGuardContext {
        loop {
            // Try the fast path: the caller owns the mutex if we manage to set the `LOCKED` bit.
            //
            // The `acquire` order matches with one of the `release` ones in `unlock`.
            if self.waiter_stack.fetch_or(LOCKED, Ordering::Acquire) & LOCKED == 0 {
                return EmptyGuardContext;
            }

            // Slow path: we'll likely need to wait, so initialise a local waiter struct.
            let mut waiter = Waiter {
                completion: Opaque::uninit(),
                next: core::ptr::null_mut(),
            };

            // SAFETY: The completion object was just allocated on the stack and is valid for
            // writes.
            unsafe { bindings::init_completion(waiter.completion.get()) };

            // Try to enqueue the waiter by pushing into onto the waiter stack. We want to do it
            // only while the mutex is locked by another thread.
            loop {
                // We use relaxed here because we're just reading the value we'll CAS later (which
                // has a stronger ordering on success).
                let mut v = self.waiter_stack.load(Ordering::Relaxed);
                if v & LOCKED == 0 {
                    // The mutex was released by another thread, so try to acquire it.
                    //
                    // The `acquire` order matches with one of the `release` ones in `unlock`.
                    v = self.waiter_stack.fetch_or(LOCKED, Ordering::Acquire);
                    if v & LOCKED == 0 {
                        return EmptyGuardContext;
                    }
                }

                waiter.next = (v & !LOCKED) as _;

                // The `release` order matches with `acquire` in `unlock` when the stack is swapped
                // out. We use release order here to ensure that the other thread can see our
                // waiter fully initialised.
                if self
                    .waiter_stack
                    .compare_exchange(
                        v,
                        (&mut waiter as *mut _ as usize) | LOCKED,
                        Ordering::Release,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    break;
                }
            }

            // Wait for the owner to lock to wake this thread up.
            //
            // SAFETY: Completion object was previously initialised with `init_completion` and
            // remains valid.
            unsafe { bindings::wait_for_completion(waiter.completion.get()) };
        }
    }

    unsafe fn unlock(&self, _: &mut EmptyGuardContext) {
        // SAFETY: The caller owns the mutex, so it is safe to manipulate the local wait queue.
        let mut waiter = unsafe { *self.waiter_queue.get() };
        loop {
            // If we have a non-empty local queue of waiters, pop the first one, release the mutex,
            // and wake it up (the popped waiter).
            if !waiter.is_null() {
                // SAFETY: The caller owns the mutex, so it is safe to manipulate the local wait
                // queue.
                unsafe { *self.waiter_queue.get() = (*waiter).next };

                // The `release` order matches with one of the `acquire` ones in `lock_noguard`.
                self.waiter_stack.fetch_and(!LOCKED, Ordering::Release);

                // Wake up the first waiter.
                //
                // SAFETY: The completion object was initialised before being added to the wait
                // stack and is only removed above, when called completed. So it is safe for
                // writes.
                unsafe { bindings::complete_all((*waiter).completion.get()) };
                return;
            }

            // Try the fast path when there are no local waiters.
            //
            // The `release` order matches with one of the `acquire` ones in `lock_noguard`.
            if self
                .waiter_stack
                .compare_exchange(LOCKED, 0, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }

            // We don't have a local queue, so pull the whole stack off, reverse it, and use it as a
            // local queue. Since we're manipulating this queue, we need to keep ownership of the
            // mutex.
            //
            // The `acquire` order matches with the `release` one in `lock_noguard` where a waiter
            // is pushed onto the stack. It ensures that we see the fully-initialised waiter.
            let mut stack =
                (self.waiter_stack.swap(LOCKED, Ordering::Acquire) & !LOCKED) as *mut Waiter;
            while !stack.is_null() {
                // SAFETY: The caller still owns the mutex, so it is safe to manipulate the
                // elements of the wait queue, which will soon become that wait queue.
                let next = unsafe { (*stack).next };

                // SAFETY: Same as above.
                unsafe { (*stack).next = waiter };

                waiter = stack;
                stack = next;
            }
        }
    }

    fn locked_data(&self) -> &UnsafeCell<T> {
        &self.data
    }
}

struct Waiter {
    completion: Opaque<bindings::completion>,
    next: *mut Waiter,
}
