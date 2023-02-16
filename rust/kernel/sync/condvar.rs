// SPDX-License-Identifier: GPL-2.0

//! A condition variable.
//!
//! This module allows Rust code to use the kernel's [`struct wait_queue_head`] as a condition
//! variable.

use super::{Guard, Lock, LockClassKey, LockInfo, NeedsLockClass};
use crate::{bindings, str::CStr, types::Opaque};
use core::{marker::PhantomPinned, pin::Pin};

/// Safely initialises a [`CondVar`] with the given name, generating a new lock class.
#[macro_export]
macro_rules! condvar_init {
    ($condvar:expr, $name:literal) => {
        $crate::init_with_lockdep!($condvar, $name)
    };
}

// TODO: `bindgen` is not generating this constant. Figure out why.
const POLLFREE: u32 = 0x4000;

/// Exposes the kernel's [`struct wait_queue_head`] as a condition variable. It allows the caller to
/// atomically release the given lock and go to sleep. It reacquires the lock when it wakes up. And
/// it wakes up when notified by another thread (via [`CondVar::notify_one`] or
/// [`CondVar::notify_all`]) or because the thread received a signal.
///
/// [`struct wait_queue_head`]: ../../../include/linux/wait.h
pub struct CondVar {
    pub(crate) wait_list: Opaque<bindings::wait_queue_head>,

    /// A condvar needs to be pinned because it contains a [`struct list_head`] that is
    /// self-referential, so it cannot be safely moved once it is initialised.
    _pin: PhantomPinned,
}

// SAFETY: `CondVar` only uses a `struct wait_queue_head`, which is safe to use on any thread.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for CondVar {}

// SAFETY: `CondVar` only uses a `struct wait_queue_head`, which is safe to use on multiple threads
// concurrently.
unsafe impl Sync for CondVar {}

impl CondVar {
    /// Constructs a new conditional variable.
    ///
    /// # Safety
    ///
    /// The caller must call `CondVar::init` before using the conditional variable.
    pub const unsafe fn new() -> Self {
        Self {
            wait_list: Opaque::uninit(),
            _pin: PhantomPinned,
        }
    }

    /// Atomically releases the given lock (whose ownership is proven by the guard) and puts the
    /// thread to sleep. It wakes up when notified by [`CondVar::notify_one`] or
    /// [`CondVar::notify_all`], or when the thread receives a signal.
    ///
    /// Returns whether there is a signal pending.
    #[must_use = "wait returns if a signal is pending, so the caller must check the return value"]
    pub fn wait<L: Lock<I>, I: LockInfo>(&self, guard: &mut Guard<'_, L, I>) -> bool {
        let lock = guard.lock;
        let wait = Opaque::<bindings::wait_queue_entry>::uninit();

        // SAFETY: `wait` points to valid memory.
        unsafe { bindings::init_wait(wait.get()) };

        // SAFETY: Both `wait` and `wait_list` point to valid memory.
        unsafe {
            bindings::prepare_to_wait_exclusive(
                self.wait_list.get(),
                wait.get(),
                bindings::TASK_INTERRUPTIBLE as _,
            )
        };

        // SAFETY: The guard is evidence that the caller owns the lock.
        unsafe { lock.unlock(&mut guard.context) };

        // SAFETY: No arguments, switches to another thread.
        unsafe { bindings::schedule() };

        guard.context = lock.lock_noguard();

        // SAFETY: Both `wait` and `wait_list` point to valid memory.
        unsafe { bindings::finish_wait(self.wait_list.get(), wait.get()) };

        // Replace when kernel::task is upstream
        //Task::current().signal_pending()
        unsafe { bindings::signal_pending(bindings::get_current()) != 0 }
    }

    /// Calls the kernel function to notify the appropriate number of threads with the given flags.
    fn notify(&self, count: i32, flags: u32) {
        // SAFETY: `wait_list` points to valid memory.
        unsafe {
            bindings::__wake_up(
                self.wait_list.get(),
                bindings::TASK_NORMAL,
                count,
                flags as _,
            )
        };
    }

    /// Wakes a single waiter up, if any. This is not 'sticky' in the sense that if no thread is
    /// waiting, the notification is lost completely (as opposed to automatically waking up the
    /// next waiter).
    pub fn notify_one(&self) {
        self.notify(1, 0);
    }

    /// Wakes all waiters up, if any. This is not 'sticky' in the sense that if no thread is
    /// waiting, the notification is lost completely (as opposed to automatically waking up the
    /// next waiter).
    pub fn notify_all(&self) {
        self.notify(0, 0);
    }

    /// Wakes all waiters up. If they were added by `epoll`, they are also removed from the list of
    /// waiters. This is useful when cleaning up a condition variable that may be waited on by
    /// threads that use `epoll`.
    pub fn free_waiters(&self) {
        self.notify(1, bindings::POLLHUP | POLLFREE);
    }
}

impl NeedsLockClass for CondVar {
    fn init(
        self: Pin<&mut Self>,
        name: &'static CStr,
        key: &'static LockClassKey,
        _: &'static LockClassKey,
    ) {
        unsafe {
            bindings::__init_waitqueue_head(self.wait_list.get(), name.as_char_ptr(), key.get())
        };
    }
}
