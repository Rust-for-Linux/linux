// SPDX-License-Identifier: GPL-2.0

//! Helper routines for polling IO addresses.
//!
//! C header: [`include/linux/iopoll.h`](../../../../include/linux/iopoll.h)

use core::mem::MaybeUninit;

use crate::{
    bindings, c_types,
    error::{Error, Result},
    might_sleep_if,
};

/// Periodically poll an address until a condition is met or a timeout occurs.
///
/// # Safety
///
/// `addr` must be non-null pointer valid io address.
pub unsafe fn read_poll_timeout<T, F: Fn(&T) -> bool>(
    op: unsafe extern "C" fn(*const c_types::c_void) -> T,
    cond: F,
    sleep_us: u32,
    timeout_us: u64,
    sleep_before_read: bool,
    addr: *const c_types::c_void,
) -> Result<T> {
    // SAFETY: FFI call.
    let timeout = unsafe { bindings::ktime_add_us(bindings::ktime_get(), timeout_us) };
    might_sleep_if!(sleep_us != 0);

    if sleep_before_read && sleep_us != 0 {
        // SAFETY: FFI call.
        unsafe { bindings::usleep_range((sleep_us >> 2) + 1, sleep_us) };
    }

    let mut val = MaybeUninit::<T>::uninit();

    loop {
        unsafe { val.as_mut_ptr().write(op(addr)) };
        if cond(unsafe { &*val.as_mut_ptr() }) {
            break;
        }

        // SAFETY: FFI call.
        if timeout_us != 0 && unsafe { bindings::ktime_compare(bindings::ktime_get(), timeout) } > 0
        {
            unsafe { val.as_mut_ptr().write(op(addr)) };
            break;
        }

        if sleep_us != 0 {
            // SAFETY: FFI call.
            unsafe { bindings::usleep_range((sleep_us >> 2) + 1, sleep_us) };
        }
    }

    if cond(unsafe { &*val.as_mut_ptr() }) {
        // SAFETY: variable is properly initialized as there is at least one write to it at loop.
        Ok(unsafe { val.assume_init() })
    } else {
        Err(Error::ETIMEDOUT)
    }
}

/// # Safety
///
/// `addr` must be non-null pointer valid io address.
pub unsafe fn readx_poll_timeout<T, F: Fn(&T) -> bool>(
    op: unsafe extern "C" fn(*const c_types::c_void) -> T,
    cond: F,
    sleep_us: u32,
    timeout_us: u64,
    addr: *const c_types::c_void,
) -> Result<T> {
    // SAFETY: `addr` is valid by the safety contract.
    unsafe { read_poll_timeout(op, cond, sleep_us, timeout_us, false, addr) }
}
