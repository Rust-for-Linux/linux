// SPDX-License-Identifier: GPL-2.0

//! Printing facilities.
//!
//! C header: [`include/linux/printk.h`](../../../../include/linux/printk.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/core-api/printk-basics.html>

use core::cmp;
use core::fmt;

use crate::bindings;
#[cfg(not(testlib))]
use crate::c_str;
use crate::c_types::{c_char, c_void};
use crate::str::CStr;

// Called from `vsprintf` with format specifier `%pA`.
#[no_mangle]
unsafe fn rust_fmt_argument(buf: *mut c_char, end: *mut c_char, ptr: *const c_void) -> *mut c_char {
    use fmt::Write;

    // Use `usize` to use `saturating_*` functions.
    struct Writer {
        buf: usize,
        end: usize,
    }

    impl Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            // `buf` value after writing `len` bytes. This does not have to be bounded
            // by `end`, but we don't want it to wrap around to 0.
            let buf_new = self.buf.saturating_add(s.len());

            // Amount that we can copy. `saturating_sub` ensures we get 0 if
            // `buf` goes past `end`.
            let len_to_copy = cmp::min(buf_new, self.end).saturating_sub(self.buf);

            // SAFETY: In any case, `buf` is non-null and properly aligned.
            // If `len_to_copy` is non-zero, then we know `buf` has not past
            // `end` yet and so is valid.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    s.as_bytes().as_ptr(),
                    self.buf as *mut u8,
                    len_to_copy,
                )
            };

            self.buf = buf_new;
            Ok(())
        }
    }

    let mut w = Writer {
        buf: buf as _,
        end: end as _,
    };
    let _ = w.write_fmt(unsafe { *(ptr as *const fmt::Arguments<'_>) });
    w.buf as _
}

/// Log level of the kernel's [`printk`].
///
/// [`printk`]: ../../../../include/linux/printk.h
#[derive(Clone, Copy)]
pub struct LogLevel(&'static [u8]);

impl LogLevel {
    /// Correspond to kernel's `KERN_EMERG` log level.
    pub const EMERG: Self = Self(bindings::KERN_EMERG);

    /// Correspond to kernel's `KERN_ALERT` log level.
    pub const ALERT: Self = Self(bindings::KERN_ALERT);

    /// Correspond to kernel's `KERN_CRIT` log level.
    pub const CRIT: Self = Self(bindings::KERN_CRIT);

    /// Correspond to kernel's `KERN_ERR` log level.
    pub const ERR: Self = Self(bindings::KERN_ERR);

    /// Correspond to kernel's `KERN_WARNING` log level.
    pub const WARNING: Self = Self(bindings::KERN_WARNING);

    /// Correspond to kernel's `KERN_NOTICE` log level.
    pub const NOTICE: Self = Self(bindings::KERN_NOTICE);

    /// Correspond to kernel's `KERN_INFO` log level.
    pub const INFO: Self = Self(bindings::KERN_INFO);

    /// Correspond to kernel's `KERN_DEBUG` log level.
    pub const DEBUG: Self = Self(bindings::KERN_DEBUG);

    /// Correspond to kernel's `KERN_CONT` log level.
    pub const CONT: Self = Self(bindings::KERN_CONT);
}

/// Trait for types that can be used as the `target` field or printk macro family.
pub trait LogTarget {
    /// Log with this target.
    fn printk(&self, lvl: LogLevel, args: fmt::Arguments<'_>);
}

impl<T: ?Sized + LogTarget> LogTarget for &T {
    #[inline]
    fn printk(&self, lvl: LogLevel, args: fmt::Arguments<'_>) {
        (*self).printk(lvl, args);
    }
}

impl LogTarget for () {
    #[inline]
    #[cfg(not(testlib))]
    fn printk(&self, lvl: LogLevel, args: fmt::Arguments<'_>) {
        // SAFETY: The format string is fixed.
        unsafe {
            bindings::printk(c_str!("%s%pA").as_char_ptr(), lvl.0.as_ptr(), &args);
        }
    }

    #[cfg(testlib)]
    fn printk(&self, _lvl: LogLevel, _args: fmt::Arguments<'_>) {}
}

impl LogTarget for CStr {
    #[inline]
    #[cfg(not(testlib))]
    fn printk(&self, lvl: LogLevel, args: fmt::Arguments<'_>) {
        // SAFETY: The format string is fixed.
        unsafe {
            bindings::printk(
                c_str!("%s%s: %pA").as_char_ptr(),
                lvl.0.as_ptr(),
                self.as_char_ptr(),
                &args,
            );
        }
    }

    #[cfg(testlib)]
    fn printk(&self, _lvl: LogLevel, _args: fmt::Arguments<'_>) {}
}

/// Prints a message with the specified log level.
///
/// Equivalent to the kernel's [`printk`].
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`printk`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.printk
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::print::*;
/// # use kernel::printk;
/// printk!(LogLevel::NOTICE, "hello {}\n", "there");
/// ```
#[cfg(not(testlib))]
#[macro_export]
macro_rules! printk (
    (target: $target:expr, $lvl:expr, $($arg:tt)+) => {{
        $crate::print::LogTarget::printk(
            &($target),
            $lvl,
            format_args!($($arg)*)
        )
    }};
    ($lvl:expr, $($arg:tt)+) => {{
        $crate::printk!(target: crate::__LOG_PREFIX, $lvl, $($arg)*)
    }}
);

/// Stub for doctests
#[cfg(testlib)]
#[macro_export]
macro_rules! printk (
    (target: $target:expr, $lvl:expr, $($arg:tt)+) => {{
        $crate::print::LogTarget::printk(
            &($target),
            $lvl,
            format_args!($($arg)*)
        )
    }};
    ($lvl:expr, $($arg:tt)+) => {{
        $crate::printk!(target: $crate::c_str!(""), $lvl, $($arg)*)
    }}
);

// We could use a macro to generate these macros. However, doing so ends
// up being a bit ugly: it requires the dollar token trick to escape `$` as
// well as playing with the `doc` attribute. Furthermore, they cannot be easily
// imported in the prelude due to [1]. So, for the moment, we just write them
// manually, like in the C side; while keeping most of the logic in another
// macro, i.e. [`printk`].
//
// [1]: https://github.com/rust-lang/rust/issues/52234

/// Prints an emergency-level message (level 0).
///
/// Use this level if the system is unusable.
///
/// Equivalent to the kernel's [`pr_emerg`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_emerg`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_emerg
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_emerg!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_emerg (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::EMERG, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::EMERG, $($arg)+)
    )
);

/// Prints an alert-level message (level 1).
///
/// Use this level if action must be taken immediately.
///
/// Equivalent to the kernel's [`pr_alert`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_alert`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_alert
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_alert!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_alert (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::ALERT, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::ALERT, $($arg)+)
    )
);

/// Prints a critical-level message (level 2).
///
/// Use this level for critical conditions.
///
/// Equivalent to the kernel's [`pr_crit`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_crit`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_crit
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_crit!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_crit (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::CRIT, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::CRIT, $($arg)+)
    )
);

/// Prints an error-level message (level 3).
///
/// Use this level for error conditions.
///
/// Equivalent to the kernel's [`pr_err`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_err`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_err
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_err!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_err (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::ERR, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::ERR, $($arg)+)
    )
);

/// Prints a warning-level message (level 4).
///
/// Use this level for warning conditions.
///
/// Equivalent to the kernel's [`pr_warn`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_warn`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_warn
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_warn!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_warn (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::WARNING, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::WARNING, $($arg)+)
    )
);

/// Prints a notice-level message (level 5).
///
/// Use this level for normal but significant conditions.
///
/// Equivalent to the kernel's [`pr_notice`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_notice`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_notice
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_notice!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_notice (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::NOTICE, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::NOTICE, $($arg)+)
    )
);

/// Prints an info-level message (level 6).
///
/// Use this level for informational messages.
///
/// Equivalent to the kernel's [`pr_info`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_info`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_info
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_info!("hello {}\n", "there");
/// ```
#[macro_export]
#[doc(alias = "print")]
macro_rules! pr_info (
    (target: $target:expr, $($arg:tt)+) => (
        $crate::printk!(target: $target, $crate::print::LogLevel::INFO, $($arg)+)
    );
    ($($arg:tt)+) => (
        $crate::printk!($crate::print::LogLevel::INFO, $($arg)+)
    )
);

/// Prints a debug-level message (level 7).
///
/// Use this level for debug messages.
///
/// Equivalent to the kernel's [`pr_debug`] macro, except that it doesn't support dynamic debug
/// yet.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_debug`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_debug
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_debug!("hello {}\n", "there");
/// ```
#[macro_export]
#[doc(alias = "print")]
macro_rules! pr_debug (
    (target: $target:expr, $($arg:tt)+) => (
        if cfg!(debug_assertions) {
            $crate::printk!(target: $target, $crate::print::LogLevel::DEBUG, $($arg)+)
        }
    );
    ($($arg:tt)+) => (
        if cfg!(debug_assertions) {
            $crate::printk!($crate::print::LogLevel::DEBUG, $($arg)+)
        }
    )
);

/// Continues a previous log message in the same line.
///
/// Use only when continuing a previous `pr_*!` macro (e.g. [`pr_info!`]).
///
/// Equivalent to the kernel's [`pr_cont`] macro.
///
/// Use the [`format!`] syntax. See [`std::fmt`] for more information.
///
/// [`pr_cont`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_cont
/// [`format!`]: alloc::format!
/// [`std::fmt`]: https://doc.rust-lang.org/std/fmt/index.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::pr_cont;
/// pr_info!("hello");
/// pr_cont!(" {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_cont (
    ($($arg:tt)*) => {{
        $crate::print::LogTarget::printk(&(), $crate::print::LogLevel::CONT, format_args!($($arg)*))
    }}
);
