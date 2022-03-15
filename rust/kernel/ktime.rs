// SPDX-License-Identifier: GPL-2.0

//! Ktime - nanosecond-resolution time format.
//!
//! C header: [`include/linux/ktime.h`](../../../../include/linux/ktime.h)

use crate::bindings;
use core::cmp::Ordering;

/// Represents `ktime_t` type.
pub struct Ktime(bindings::ktime_t);

/// Get current time value.
pub fn get() -> Ktime {
    // SAFETY: FFI call.
    unsafe { Ktime(bindings::ktime_get()) }
}

impl Ktime {
    /// Add microseconds to ktime.
    pub fn add_us(&self, usec: u64) -> Ktime {
        // SAFETY: FFI call.
        unsafe { Ktime(bindings::ktime_add_us(self.0, usec)) }
    }
}

impl PartialEq for Ktime {
    fn eq(&self, other: &Self) -> bool {
        ktime_compare(self, other) == Ordering::Equal
    }
}

impl PartialOrd for Ktime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(ktime_compare(self, other))
    }
}

/// Compare ktime values.
fn ktime_compare(cmp1: &Ktime, cmp2: &Ktime) -> Ordering {
    // SAFETY: FFI call.
    let ret = unsafe { bindings::ktime_compare(cmp1.0, cmp2.0) };
    ret.cmp(&0)
}
