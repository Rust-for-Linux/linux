// SPDX-License-Identifier: GPL-2.0

//! Reset controller abstractions.
//!
//! C header: [`include/linux/reset.h`](../../../../include/linux/reset.h)

use crate::{bindings, to_result, Result};

/// Represents `struct reset_control *`.
///
/// # Invariants
///
/// The pointer is valid.
pub struct Reset(*mut bindings::reset_control);

// SAFETY: ‘Reset‘ is not restricted to a single thread so it is safe
// to move it between threads
unsafe impl Send for Reset {}

impl Reset {
    /// Creates a new reset_control structure from a raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be valid.
    pub unsafe fn new(rst: *mut bindings::reset_control) -> Self {
        Self(rst)
    }

    /// Resets the reset control
    pub fn control_reset(&self) -> Result {
        // SAFETY: The pointer is valid by the type invariant.
        to_result(unsafe { bindings::reset_control_reset(self.0) })
    }
}

impl Drop for Reset {
    fn drop(&mut self) {
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::reset_control_put(self.0) };
    }
}
