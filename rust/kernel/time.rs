// SPDX-License-Identifier: GPL-2.0

//! Timekeeping functions.
//!
//! C header: [`include/linux/ktime.h`](../../../../include/linux/ktime.h)
//! C header: [`include/linux/timekeeping.h`](../../../../include/linux/timekeeping.h)

use crate::bindings;
use core::time::Duration;

/// Returns the kernel time elapsed since boot, excluding time spent sleeping, as a [`Duration`].
pub fn ktime_get() -> Duration {
    Duration::from_nanos(unsafe { bindings::ktime_get() }.try_into().unwrap())
}

/// Returns the kernel time elapsed since boot, including time spent sleeping, as a [`Duration`].
pub fn ktime_get_boottime() -> Duration {
    Duration::from_nanos(
        unsafe { bindings::ktime_get_with_offset(bindings::tk_offsets_TK_OFFS_BOOT) }
            .try_into()
            .unwrap(),
    )
}
