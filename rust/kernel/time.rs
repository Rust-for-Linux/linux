// SPDX-License-Identifier: GPL-2.0

//! Time representation in the kernel.
//!
//! C headers: [`include/linux/time64.h`](../../include/linux/time64.h)

use crate::{bindings, error::code::*, error::Result};

/// A [`Timespec`] instance at the Unix epoch.
pub const UNIX_EPOCH: Timespec = Timespec {
    t: bindings::timespec64 {
        tv_sec: 0,
        tv_nsec: 0,
    },
};

/// A timestamp.
#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Timespec {
    t: bindings::timespec64,
}

impl Timespec {
    /// Creates a new timestamp.
    ///
    /// `sec` is the number of seconds since the Unix epoch. `nsec` is the number of nanoseconds
    /// within that second.
    pub fn new(sec: u64, nsec: u32) -> Result<Self> {
        if nsec >= 1000000000 {
            return Err(EDOM);
        }

        Ok(Self {
            t: bindings::timespec64 {
                tv_sec: sec.try_into()?,
                tv_nsec: nsec.try_into()?,
            },
        })
    }
}

impl From<Timespec> for bindings::timespec64 {
    fn from(v: Timespec) -> Self {
        v.t
    }
}
