// SPDX-License-Identifier: GPL-2.0

//! Delay routines
//!
//! C header: [`include/linux/delay.h`](../../../../include/linux/delay.h)

use crate::bindings;

/// Sleep between two microsecond values.
pub fn usleep_range(min: usize, max: usize) {
    // SAFETY: FFI call.
    unsafe { bindings::usleep_range(min as _, max as _) };
}
