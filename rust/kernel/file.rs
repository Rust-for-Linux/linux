// SPDX-License-Identifier: GPL-2.0

//! Files and file descriptors.
//!
//! C headers: [`include/linux/fs.h`](../../../../include/linux/fs.h) and
//! [`include/linux/file.h`](../../../../include/linux/file.h)

use crate::bindings;
use core::ptr::NonNull;

/// Wraps the kernel's `struct file`.
///
/// # Invariants
///
/// The pointer [`File::ptr`] is valid.
pub struct File {
    pub(crate) ptr: NonNull<bindings::file>,
}

impl File {
    /// Constructs a new [`struct file`] wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be valid for the lifetime of the object.
    pub(crate) unsafe fn from_ptr(ptr: NonNull<bindings::file>) -> File {
        // INVARIANTS: the safety contract ensures the type invariant will hold.
        File { ptr }
    }

    /// Returns the current seek/cursor/pointer position (`struct file::f_pos`).
    pub fn pos(&self) -> u64 {
        // SAFETY: `File::ptr` is guaranteed to be valid by the type invariants.
        unsafe { self.ptr.as_ref().f_pos as u64 }
    }

    /// Returns whether the file is in blocking mode.
    pub fn is_blocking(&self) -> bool {
        // SAFETY: `File::ptr` is guaranteed to be valid by the type invariants.
        unsafe { self.ptr.as_ref().f_flags & bindings::O_NONBLOCK == 0 }
    }
}
