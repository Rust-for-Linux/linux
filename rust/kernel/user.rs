// SPDX-License-Identifier: GPL-2.0

//! User-space related functions.
use crate::error::{code::*, Result};

/// A writer to userspace memory.
pub struct Writer {
    ptr: *mut u8,
    len: usize,
}

impl Writer {
    pub(crate) fn new(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// Writes all of `data` into user memory.
    pub fn write_all(&mut self, data: &[u8]) -> Result {
        let len = data.len();
        if len > self.len {
            return Err(EFAULT);
        }

        // SAFETY: `len <= self.len` is checked above, ensuring `self.ptr` points to
        // at least `len` bytes of valid userspace memory, and `data` is a valid slice.
        let pending = unsafe {
            bindings::copy_to_user(
                self.ptr.cast::<core::ffi::c_void>(),
                data.as_ptr().cast::<core::ffi::c_void>(),
                data.len(),
            )
        };
        if pending != 0 {
            return Err(EFAULT);
        }

        self.ptr = self.ptr.wrapping_add(len);
        self.len -= len;

        Ok(())
    }
}
