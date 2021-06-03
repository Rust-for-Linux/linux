// SPDX-License-Identifier: GPL-2.0

//! Random numbers.
//!
//! C header: [`include/linux/random.h`](../../../../include/linux/random.h)

use core::convert::TryInto;

use crate::{
    bindings, c_types,
    error::{self, from_kernel_int_result},
};

/// Fills a byte slice with random bytes generated from the kernel's CSPRNG.
///
/// Ensures that the CSPRNG has been seeded before generating any random bytes,
/// and will block until it is ready.
pub fn getrandom(dest: &mut [u8]) -> error::Result {
    let res = unsafe { bindings::wait_for_random_bytes() };
    // SAFETY: `bindings::wait_for_random_bytes()` returns zero on success,
    // or a valid negative `errno` on error.
    unsafe {
        from_kernel_int_result(res)?;
    }

    unsafe {
        bindings::get_random_bytes(
            dest.as_mut_ptr() as *mut c_types::c_void,
            dest.len().try_into()?,
        );
    }
    Ok(())
}

/// Fills a byte slice with random bytes generated from the kernel's CSPRNG.
///
/// If the CSPRNG is not yet seeded, returns an `Err(EAGAIN)` immediately.
pub fn getrandom_nonblock(dest: &mut [u8]) -> error::Result {
    if !unsafe { bindings::rng_is_initialized() } {
        return Err(error::Error::EAGAIN);
    }
    getrandom(dest)
}

/// Contributes the contents of a byte slice to the kernel's entropy pool.
///
/// Does *not* credit the kernel entropy counter though.
pub fn add_randomness(data: &[u8]) {
    unsafe {
        bindings::add_device_randomness(
            data.as_ptr() as *const c_types::c_void,
            data.len().try_into().unwrap(),
        );
    }
}
