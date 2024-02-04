// SPDX-License-Identifier: GPL-2.0

//! Time related primitives.
//!
//! This module contains the kernel APIs related to time and timers that
//! have been ported or wrapped for usage by Rust code in the kernel.

/// The time unit of Linux kernel. One jiffy equals (1/HZ) second.
pub type Jiffies = core::ffi::c_ulong;

/// Jiffies, but with a fixed width of 32bit.
pub type Jiffies32 = u32;

/// The millisecond time unit.
pub type Msecs = core::ffi::c_uint;

/// Milliseconds per second.
pub const MSEC_PER_SEC: Msecs = 1000;

/// The milliseconds time unit with a fixed width of 32bit.
///
/// This is used in networking.
pub type Msecs32 = u32;

/// The microseconds time unit.
pub type Usecs = u64;

/// Microseconds per millisecond.
pub const USEC_PER_MSEC: Usecs = 1000;

/// Microseconds per second.
pub const USEC_PER_SEC: Usecs = 1_000_000;

/// The microseconds time unit with a fixed width of 32bit.
///
/// This is used in networking.
pub type Usecs32 = u32;

/// The nanosecond time unit.
pub type Nsecs = u64;

/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: Nsecs = 1000;

/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: Nsecs = 1_000_000;

/// Converts milliseconds to jiffies.
#[inline]
pub fn msecs_to_jiffies(msecs: Msecs) -> Jiffies {
    // SAFETY: The `__msecs_to_jiffies` function is always safe to call no
    // matter what the argument is.
    unsafe { bindings::__msecs_to_jiffies(msecs) }
}

/// Converts jiffies to milliseconds.
#[inline]
pub fn jiffies_to_msecs(jiffies: Jiffies) -> Msecs {
    // SAFETY: The `__msecs_to_jiffies` function is always safe to call no
    // matter what the argument is.
    unsafe { bindings::jiffies_to_msecs(jiffies) }
}

/// Returns the current time in 32bit jiffies.
#[inline]
pub fn jiffies32() -> Jiffies32 {
    // SAFETY: It is always atomic to read the lower 32bit of jiffies.
    unsafe { bindings::jiffies as u32 }
}

/// Returns the time elapsed since system boot, in nanoseconds. Does include the
/// time the system was suspended.
#[inline]
pub fn ktime_get_boot_fast_ns() -> Nsecs {
    // SAFETY: FFI call without safety requirements.
    unsafe { bindings::ktime_get_boot_fast_ns() }
}

/// Returns the time elapsed since system boot, in 32bit microseconds. Does
/// include the time the system was suspended.
#[inline]
pub fn ktime_get_boot_fast_us32() -> Usecs32 {
    (ktime_get_boot_fast_ns() / NSEC_PER_USEC) as Usecs32
}

/// Returns the time elapsed since system boot, in 32bit milliseconds. Does
/// include the time the system was suspended.
#[inline]
pub fn ktime_get_boot_fast_ms32() -> Msecs32 {
    (ktime_get_boot_fast_ns() / NSEC_PER_MSEC) as Msecs32
}
