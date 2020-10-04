// SPDX-License-Identifier: GPL-2.0

use core::cmp;
use core::fmt;

use crate::bindings;
use crate::c_types::c_int;

pub use crate::bindings::{
    KERN_ALERT, KERN_CRIT, KERN_DEBUG, KERN_EMERG, KERN_ERR, KERN_INFO, KERN_NOTICE, KERN_WARNING,
};

const LEVEL_LEN: usize = 3;

#[doc(hidden)]
pub fn printk(s: &[u8], level: &'static [u8; LEVEL_LEN]) {
    // Don't copy the trailing NUL from `KERN_INFO`.
    let mut fmt_str = [0; LEVEL_LEN - 1 + b"%.*s\0".len()];
    fmt_str[..LEVEL_LEN - 1].copy_from_slice(&level[..LEVEL_LEN - 1]);
    fmt_str[LEVEL_LEN - 1..].copy_from_slice(b"%.*s\0");

    // TODO: I believe printk never fails
    unsafe { bindings::printk(fmt_str.as_ptr() as _, s.len() as c_int, s.as_ptr()) };
}

// From kernel/print/printk.c
const LOG_LINE_MAX: usize = 1024 - 32;

#[doc(hidden)]
pub struct LogLineWriter {
    data: [u8; LOG_LINE_MAX],
    pos: usize,
}

#[allow(clippy::new_without_default)]
impl LogLineWriter {
    pub fn new() -> LogLineWriter {
        LogLineWriter {
            data: [0u8; LOG_LINE_MAX],
            pos: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.pos]
    }
}

impl fmt::Write for LogLineWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let copy_len = cmp::min(LOG_LINE_MAX - self.pos, s.as_bytes().len());
        self.data[self.pos..self.pos + copy_len].copy_from_slice(&s.as_bytes()[..copy_len]);
        self.pos += copy_len;
        Ok(())
    }
}

/// [`kprintln!`] prints to the kernel console with a given level.
/// If no level is given, it will default to `KERN_INFO`.
#[macro_export]
macro_rules! kprintln {
    () => ({
        kprintln!(level: $crate::printk::KERN_INFO);
    });
    (level: $level:expr) => ({
        $crate::printk::printk("\n".as_bytes(), $level);
    });
    ($msg:expr) => ({
        kprintln!(level: $crate::printk::KERN_INFO, $msg);
    });
    (level: $level:expr, $msg:expr) => ({
        $crate::printk::printk(concat!($msg, "\n").as_bytes(), $level);
    });
    (level: $level:expr, $fmt:expr, $($arg:tt)*) => ({
        use ::core::fmt;
        let mut writer = $crate::printk::LogLineWriter::new();
        let _ = fmt::write(&mut writer, format_args!(concat!($fmt, "\n"), $($arg)*)).unwrap();
        $crate::printk::printk(writer.as_bytes(), $crate::printk::KERN_INFO);
    });
    ($fmt:expr, $($arg:tt)*) => ({
        kprintln!(level: $crate::printk::KERN_INFO, $fmt, $($arg)*);
    });
}

/// [`println!`] functions the same as it does in `std`, except instead of
/// printing to `stdout`, it writes to the kernel console at the `KERN_INFO`
/// level.
///
/// [`println!`]: https://doc.rust-lang.org/stable/std/macro.println.html
#[macro_export]
macro_rules! println {
    () => ({
        kprintln!(level: $crate::printk::KERN_INFO);
    });
    ($msg:expr) => ({
        kprintln!(level: $crate::printk::KERN_INFO, $msg);
    });
    ($fmt:expr, $($arg:tt)*) => ({
        kprintln!(level: $crate::printk::KERN_INFO, $fmt, $($arg)*);
    });
}

/// [`eprintln!`] functions the same as it does in `std`, except instead of
/// printing to `stderr`, it writes to the kernel console at the `KERN_ERR`
/// level.
///
/// [`eprintln!`]: https://doc.rust-lang.org/stable/std/macro.eprintln.html
#[macro_export]
macro_rules! eprintln {
    () => ({
        kprintln!(level: $crate::printk::KERN_ERR);
    });
    ($msg:expr) => ({
        kprintln!(level: $crate::printk::KERN_ERR, $msg);
    });
    ($fmt:expr, $($arg:tt)*) => ({
        kprintln!(level: $crate::printk::KERN_ERR, $msg, $($arg)*);
    });
}
