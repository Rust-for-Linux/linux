// SPDX-License-Identifier: GPL-2.0

//! Files and file descriptors.
//!
//! C headers: [`include/linux/fs.h`](../../../../include/linux/fs.h) and
//! [`include/linux/file.h`](../../../../include/linux/file.h)

use crate::{
    bindings,
    error::{from_kernel_int_result_uint, Error},
    Result,
};
use core::{mem::ManuallyDrop, ops::Deref};

/// Wraps the kernel's `struct file`.
///
/// # Invariants
///
/// The pointer [`File::ptr`] is non-null and valid. Its reference count is also non-zero.
pub struct File {
    pub(crate) ptr: *mut bindings::file,
}

impl File {
    /// Constructs a new [`struct file`] wrapper from a file descriptor.
    ///
    /// The file descriptor belongs to the current process.
    pub fn from_fd(fd: u32) -> Result<Self> {
        // SAFETY: FFI call, there are no requirements on `fd`.
        let ptr = unsafe { bindings::fget(fd) };
        if ptr.is_null() {
            return Err(Error::EBADF);
        }

        // INVARIANTS: We checked that `ptr` is non-null, so it is valid. `fget` increments the ref
        // count before returning.
        Ok(Self { ptr })
    }

    /// Returns the current seek/cursor/pointer position (`struct file::f_pos`).
    pub fn pos(&self) -> u64 {
        // SAFETY: `File::ptr` is guaranteed to be valid by the type invariants.
        unsafe { (*self.ptr).f_pos as u64 }
    }

    /// Returns whether the file is in blocking mode.
    pub fn is_blocking(&self) -> bool {
        // SAFETY: `File::ptr` is guaranteed to be valid by the type invariants.
        unsafe { (*self.ptr).f_flags & bindings::O_NONBLOCK == 0 }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: The type invariants guarantee that `File::ptr` has a non-zero reference count.
        unsafe { bindings::fput(self.ptr) };
    }
}

/// A wrapper for [`File`] that doesn't automatically decrement that refcount when dropped.
///
/// We need the wrapper because [`ManuallyDrop`] alone would allow callers to call
/// [`ManuallyDrop::into_inner`]. This would allow an unsafe sequence to be triggered without
/// `unsafe` blocks because it would trigger an unbalanced call to `fput`.
///
/// # Invariants
///
/// The wrapped [`File`] remains valid for the lifetime of the object.
pub(crate) struct FileRef(ManuallyDrop<File>);

impl FileRef {
    /// Constructs a new [`struct file`] wrapper that doesn't change its reference count.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::file) -> Self {
        Self(ManuallyDrop::new(File { ptr }))
    }
}

impl Deref for FileRef {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// A file descriptor reservation.
///
/// This allows the creation of a file descriptor in two steps: first, we reserve a slot for it,
/// then we commit or drop the reservation. The first step may fail (e.g., the current process ran
/// out of available slots), but commit and drop never fail (and are mutually exclusive).
pub struct FileDescriptorReservation {
    fd: u32,
}

impl FileDescriptorReservation {
    /// Creates a new file descriptor reservation.
    pub fn new(flags: u32) -> Result<Self> {
        let fd = unsafe { bindings::get_unused_fd_flags(flags) };
        // SAFETY: `bindings::get_unused_fd_flags()` returns a non-negative
        // `fd` on success, or a valid negative `errno` on error.
        let fd = unsafe { from_kernel_int_result_uint(fd)? };
        Ok(Self { fd: fd as _ })
    }

    /// Returns the file descriptor number that was reserved.
    pub fn reserved_fd(&self) -> u32 {
        self.fd
    }

    /// Commits the reservation.
    ///
    /// The previously reserved file descriptor is bound to `file`.
    pub fn commit(self, file: File) {
        // SAFETY: `self.fd` was previously returned by `get_unused_fd_flags`, and `file.ptr` is
        // guaranteed to have an owned ref count by its type invariants.
        unsafe { bindings::fd_install(self.fd, file.ptr) };

        // `fd_install` consumes both the file descriptor and the file reference, so we cannot run
        // the destructors.
        core::mem::forget(self);
        core::mem::forget(file);
    }
}

impl Drop for FileDescriptorReservation {
    fn drop(&mut self) {
        // SAFETY: `self.fd` was returned by a previous call to `get_unused_fd_flags`.
        unsafe { bindings::put_unused_fd(self.fd) };
    }
}
