// SPDX-License-Identifier: GPL-2.0

//! Utilities for working with `struct poll_table`.

use crate::{
    bindings,
    file::File,
    prelude::*,
    sync::{CondVar, LockClassKey},
    types::Opaque,
};
use core::ops::Deref;

/// Creates a [`PollCondVar`] initialiser with the given name and a newly-created lock class.
#[macro_export]
macro_rules! new_poll_condvar {
    ($($name:literal)?) => {
        $crate::file::PollCondVar::new($crate::optional_name!($($name)?), $crate::static_lock_class!())
    };
}

/// Wraps the kernel's `struct poll_table`.
#[repr(transparent)]
pub struct PollTable(Opaque<bindings::poll_table>);

impl PollTable {
    /// Creates a reference to a [`PollTable`] from a valid pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that for the duration of 'a, the pointer will point at a valid poll
    /// table, and that it is only accessed via the returned reference.
    pub unsafe fn from_ptr<'a>(ptr: *mut bindings::poll_table) -> &'a mut PollTable {
        // SAFETY: The safety requirements guarantee the validity of the dereference, while the
        // `PollTable` type being transparent makes the cast ok.
        unsafe { &mut *ptr.cast() }
    }

    fn get_qproc(&self) -> bindings::poll_queue_proc {
        let ptr = self.0.get();
        // SAFETY: The `ptr` is valid because it originates from a reference, and the `_qproc`
        // field is not modified concurrently with this call since we have an immutable reference.
        unsafe { (*ptr)._qproc }
    }

    /// Register this [`PollTable`] with the provided [`PollCondVar`], so that it can be notified
    /// using the condition variable.
    pub fn register_wait(&mut self, file: &File, cv: &PollCondVar) {
        if let Some(qproc) = self.get_qproc() {
            // SAFETY: The pointers to `self` and `file` are valid because they are references.
            //
            // Before the wait list is destroyed, the destructor of `PollCondVar` will clear
            // everything in the wait list, so the wait list is not used after it is freed.
            unsafe { qproc(file.as_ptr() as _, cv.wait_list.get(), self.0.get()) };
        }
    }
}

/// A wrapper around [`CondVar`] that makes it usable with [`PollTable`].
///
/// # Invariant
///
/// If `needs_synchronize_rcu` is false, then there is nothing registered with `register_wait`.
///
/// [`CondVar`]: crate::sync::CondVar
#[pin_data(PinnedDrop)]
pub struct PollCondVar {
    #[pin]
    inner: CondVar,
}

impl PollCondVar {
    /// Constructs a new condvar initialiser.
    pub fn new(name: &'static CStr, key: &'static LockClassKey) -> impl PinInit<Self> {
        pin_init!(Self {
            inner <- CondVar::new(name, key),
        })
    }
}

// Make the `CondVar` methods callable on `PollCondVar`.
impl Deref for PollCondVar {
    type Target = CondVar;

    fn deref(&self) -> &CondVar {
        &self.inner
    }
}

#[pinned_drop]
impl PinnedDrop for PollCondVar {
    fn drop(self: Pin<&mut Self>) {
        // Clear anything registered using `register_wait`.
        //
        // SAFETY: The pointer points at a valid wait list.
        unsafe { bindings::__wake_up_pollfree(self.inner.wait_list.get()) };

        // Wait for epoll items to be properly removed.
        //
        // SAFETY: Just an FFI call.
        unsafe { bindings::synchronize_rcu() };
    }
}
