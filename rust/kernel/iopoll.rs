// SPDX-License-Identifier: GPL-2.0

//! Helper routines for polling i/o addresses.
//!
//! C header: [`include/linux/iopoll.h`](../../../../include/linux/iopoll.h)
use crate::{
    c_types, delay,
    error::{Error, Result},
    ktime, might_sleep_if,
};

/// Periodically poll an address until a condition is met or a timeout occurs.
///
/// - `op`: poll function, takes `args` as its arguments,
/// - `cond`: break condition,
/// - `sleep_us`: maximum time to sleep between reads in us (0
///    tight-loops). Should be less than ~20ms since usleep_range
///    is used, see [documentation],
/// - `timeout_us`: timeout in `us`, 0 means never timeout,
/// - `sleep_before_read`: when true, sleep for `sleep_us` before read,
/// - `args`: arguments for `op` poll function.
///
/// Returns the last read value or ETIMEDOUT upon a timeout.
/// Must not be called from atomic context if `sleep_us` or `timeout_us` are used.
///
/// [documentation]: [Documentation/timers/timers-howto.rst]
///
/// # Safety
///
/// `op` must be non-null function pointer or other callable object.
macro_rules! read_poll_timeout {
    ($op:ident, $cond:expr, $sleep_us:expr, $timeout_us:expr,
     $sleep_before_read:literal, $( $args:expr ),*) => {
        {
            let sleep_us: usize = $sleep_us;
            let timeout_us: u64 = $timeout_us;
            let sleep_before_read: bool = $sleep_before_read;
            let timeout = ktime::get().add_us(timeout_us);
            might_sleep_if!(sleep_us != 0);

            if sleep_before_read && sleep_us != 0 {
                delay::usleep_range((sleep_us >> 2) + 1, sleep_us);
            }

            let val = loop {
                // SAFETY: `op` is valid by the safety contract.
                let val = unsafe { $op($( $args ),*) };
                if $cond(&val) {
                    break val;
                }

                if timeout_us != 0 && ktime::get() > timeout {
                    // SAFETY: `op` is valid by the safety contract.
                    break unsafe { $op($( $args ),*) };
                }

                if sleep_us != 0 {
                    delay::usleep_range((sleep_us >> 2) + 1, sleep_us);
                }
            };

            if $cond(&val) {
                Ok(val)
            } else {
                Err(Error::ETIMEDOUT)
            }
        }
    }
}

/// Periodically polls an address until a condition is met or a timeout occurs.
///
/// - `op`: poll function, takes `args` as its arguments,
/// - `cond`: break condition,
/// - `sleep_us`: maximum time to sleep between reads in us (0
///    tight-loops). Should be less than ~20ms since usleep_range
///    is used, see [documentation],
/// - `timeout_us`: timeout in `us`, 0 means never timeout,
/// - `args`: arguments for `op` poll function.
///
/// [documentation]: [Documentation/timers/timers-howto.rst]
///
/// # Safety
///
/// `op` must be non-null function pointer or other callable object.
/// `addr` must be non-null i/o address.
pub unsafe fn readx_poll_timeout<T, F: Fn(&T) -> bool>(
    op: unsafe extern "C" fn(*const c_types::c_void) -> T,
    cond: F,
    sleep_us: usize,
    timeout_us: u64,
    addr: *const c_types::c_void,
) -> Result<T> {
    // SAFETY: `op` and `addr` are valid by the safety contract.
    read_poll_timeout!(op, cond, sleep_us, timeout_us, false, addr)
}
