// SPDX-License-Identifier: GPL-2.0

//! Printing facilities.
//!
//! C header: [`include/linux/printk.h`](../../../../include/linux/printk.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/core-api/printk-basics.html>

use core::cmp;
use core::fmt;

use crate::bindings;
use crate::c_types::{c_char, c_void};

pub use bindings::___ratelimit;
pub use bindings::__printk_ratelimit;
pub use bindings::ratelimit_state;

#[doc(hidden)]
extern "C" {
    /// This function initializes ratelimit_state with DEFAULT_RATELIMIT_INTERVAL
    /// and DEFAULT_RATELIMIT_BURST.
    /// It should only be used inside [`print_macro_ratelimited`] macro.
    #[allow(improper_ctypes)]
    pub fn rust_helper_ratelimit_state_init(rs: *mut bindings::ratelimit_state);
}

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

/// Format strings.
///
/// Public but hidden since it should only be used from public macros.
#[doc(hidden)]
pub mod format_strings {
    use crate::bindings;

    /// The length we copy from the `KERN_*` kernel prefixes.
    const LENGTH_PREFIX: usize = 2;

    /// The length of the fixed format strings.
    pub const LENGTH: usize = 10;

    /// Generates a fixed format string for the kernel's [`_printk`].
    ///
    /// The format string is always the same for a given level, i.e. for a
    /// given `prefix`, which are the kernel's `KERN_*` constants.
    ///
    /// [`_printk`]: ../../../../include/linux/printk.h
    const fn generate(is_cont: bool, prefix: &[u8; 3]) -> [u8; LENGTH] {
        // Ensure the `KERN_*` macros are what we expect.
        assert!(prefix[0] == b'\x01');
        if is_cont {
            assert!(prefix[1] == b'c');
        } else {
            assert!(prefix[1] >= b'0' && prefix[1] <= b'7');
        }
        assert!(prefix[2] == b'\x00');

        let suffix: &[u8; LENGTH - LENGTH_PREFIX] = if is_cont {
            b"%pA\0\0\0\0\0"
        } else {
            b"%s: %pA\0"
        };

        [
            prefix[0], prefix[1], suffix[0], suffix[1], suffix[2], suffix[3], suffix[4], suffix[5],
            suffix[6], suffix[7],
        ]
    }

    // Generate the format strings at compile-time.
    //
    // This avoids the compiler generating the contents on the fly in the stack.
    //
    // Furthermore, `static` instead of `const` is used to share the strings
    // for all the kernel.
    pub static EMERG: [u8; LENGTH] = generate(false, bindings::KERN_EMERG);
    pub static ALERT: [u8; LENGTH] = generate(false, bindings::KERN_ALERT);
    pub static CRIT: [u8; LENGTH] = generate(false, bindings::KERN_CRIT);
    pub static ERR: [u8; LENGTH] = generate(false, bindings::KERN_ERR);
    pub static WARNING: [u8; LENGTH] = generate(false, bindings::KERN_WARNING);
    pub static NOTICE: [u8; LENGTH] = generate(false, bindings::KERN_NOTICE);
    pub static INFO: [u8; LENGTH] = generate(false, bindings::KERN_INFO);
    pub static DEBUG: [u8; LENGTH] = generate(false, bindings::KERN_DEBUG);
    pub static CONT: [u8; LENGTH] = generate(true, bindings::KERN_CONT);
}

/// Prints a message via the kernel's [`_printk`].
///
/// Public but hidden since it should only be used from public macros.
///
/// # Safety
///
/// The format string must be one of the ones in [`format_strings`], and
/// the module name must be null-terminated.
///
/// [`_printk`]: ../../../../include/linux/_printk.h
#[doc(hidden)]
pub unsafe fn call_printk(
    format_string: &[u8; format_strings::LENGTH],
    module_name: &[u8],
    args: fmt::Arguments<'_>,
) {
    // `_printk` does not seem to fail in any path.
    unsafe {
        bindings::_printk(
            format_string.as_ptr() as _,
            module_name.as_ptr(),
            &args as *const _ as *const c_void,
        );
    }
}

/// Prints a message via the kernel's [`_printk`] for the `CONT` level.
///
/// Public but hidden since it should only be used from public macros.
///
/// [`_printk`]: ../../../../include/linux/printk.h
#[doc(hidden)]
pub fn call_printk_cont(args: fmt::Arguments<'_>) {
    // `_printk` does not seem to fail in any path.
    //
    // SAFETY: The format string is fixed.
    unsafe {
        bindings::_printk(
            format_strings::CONT.as_ptr() as _,
            &args as *const _ as *const c_void,
        );
    }
}

/// Performs formatting and forwards the string to [`call_printk`].
///
/// Public but hidden since it should only be used from public macros.
#[doc(hidden)]
#[cfg(not(testlib))]
#[macro_export]
macro_rules! print_macro (
    // The non-continuation cases (most of them, e.g. `INFO`).
    ($format_string:path, false, $($arg:tt)+) => (
        // SAFETY: This hidden macro should only be called by the documented
        // printing macros which ensure the format string is one of the fixed
        // ones. All `__LOG_PREFIX`s are null-terminated as they are generated
        // by the `module!` proc macro or fixed values defined in a kernel
        // crate.
        unsafe {
            $crate::print::call_printk(
                &$format_string,
                crate::__LOG_PREFIX,
                format_args!($($arg)+),
            );
        }
    );

    // The `CONT` case.
    ($format_string:path, true, $($arg:tt)+) => (
        $crate::print::call_printk_cont(
            format_args!($($arg)+),
        );
    );
);

/// Same as [`print_macro`], but ratelimites messages with local ratelimit_state.
/// This emulates what [`printk_ratelimited`] macro does in C.
///
/// Public but hidden since it should only be used from public macros.
///
/// The extra parameter [`where`] is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
#[doc(hidden)]
#[cfg(not(testlib))]
#[macro_export]
macro_rules! print_macro_ratelimited (
    // The non-continuation cases (most of them, e.g. `INFO`).
    ($format_string:path, false, $where:literal, $($arg:tt)+) => ({
        static mut PRINTK_RS: core::mem::MaybeUninit<$crate::print::ratelimit_state> =
            core::mem::MaybeUninit::<$crate::print::ratelimit_state>::uninit();
        static mut PRINTK_RS_PTR: core::sync::atomic::AtomicPtr<$crate::print::ratelimit_state> =
            core::sync::atomic::AtomicPtr::<$crate::print::ratelimit_state>::new(core::ptr::null_mut());
        static mut PRINTK_RS_AVAILABLE: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);

        // SAFETY: [`PRINTK_RS_AVAILABLE`] and [`PRINTK_RS_PTR`] is used to
        // control the intialization of [`PRINTK_RS`] - a local
        // [`ratelimit_state`] initialized with [`DEFAULT_RATELIMIT_INTERVAL`]
        // and [`DEFAULT_RATELIMIT_BURST`]. All of these three variables are
        // static local variables to avoid dynamic memory allocation and no
        // lock is used because printk can be used in any circumstance.
        //
        // The hidden macro [`call_printk`] should only be called by the
        // documented printing macros which ensure the format string is one of
        // the fixed ones.
        // All `__LOG_PREFIX`s are null-terminated as they are generated
        // by the `module!` proc macro or fixed values defined in a kernel
        // crate.
        unsafe {
            if PRINTK_RS_AVAILABLE.compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::SeqCst,
                core::sync::atomic::Ordering::SeqCst
            ).is_ok() {
                // Zero out [`PRINTK_RS`] then initialize it using
                // [`rust_helper_ratelimit_state_init`] helper function.
                *PRINTK_RS.as_mut_ptr() = $crate::print::ratelimit_state::default();
                $crate::print::rust_helper_ratelimit_state_init(PRINTK_RS.as_mut_ptr());

                // Make PRINTK_RS_PTR point at PRINTK_RS so later we can call
                // [`___ratelimit`] with it.
                if PRINTK_RS_PTR.load(
                    core::sync::atomic::Ordering::SeqCst
                ).is_null() {
                    PRINTK_RS_PTR.store(
                        PRINTK_RS.as_mut_ptr(),
                        core::sync::atomic::Ordering::SeqCst
                    );
                }
            }

            // [`PRINTK_RS`] can be uninitialized under concurrent situation,
            // if so, we print message without ratelimit control.
            // Otherwise we call __ratelimit to decide if
            // enforce ratelimit.
            if PRINTK_RS_PTR.load(core::sync::atomic::Ordering::SeqCst).is_null() ||
                $crate::print::___ratelimit(
                    PRINTK_RS_PTR.load(core::sync::atomic::Ordering::SeqCst),
                    $crate::c_str!($where).as_char_ptr()
                ) != 0 {
                $crate::print::call_printk(
                    &$format_string,
                    crate::__LOG_PREFIX,
                    format_args!($($arg)+),
                );
            }
        }
    });

    // The `CONT` case.
    ($format_string:path, true, $where:literal, $($arg:tt)+) => ({
        $crate::print::call_printk_cont(
            format_args!($($arg)+),
        );
    });
);

/// Stub for doctests
#[cfg(testlib)]
#[macro_export]
macro_rules! print_macro (
    ($format_string:path, $e:expr, $($arg:tt)+) => (
        ()
    );
);

/// Stub for doctests
#[cfg(testlib)]
#[macro_export]
macro_rules! print_macro_ratelimited (
    ($format_string:path, $e:expr, $where:literal, $($arg:tt)+) => (
        ()
    );
);

// We could use a macro to generate these macros. However, doing so ends
// up being a bit ugly: it requires the dollar token trick to escape `$` as
// well as playing with the `doc` attribute. Furthermore, they cannot be easily
// imported in the prelude due to [1]. So, for the moment, we just write them
// manually, like in the C side; while keeping most of the logic in another
// macro, i.e. [`print_macro`].
//
// [1]: https://github.com/rust-lang/rust/issues/52234

/// Prints an emergency-level message (level 0).
///
/// Use this level if the system is unusable.
///
/// Equivalent to the kernel's [`pr_emerg`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_emerg`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_emerg
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_emerg!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_emerg (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::EMERG, false, $($arg)*)
    )
);

/// Prints an emergency-level message (level 0) with ratelimit.
///
/// Use this level if the system is unusable.
///
/// Equivalent to the kernel's [`pr_emerg_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_emerg`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_emerg
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_emerg_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_emerg_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::EMERG, false, $where, $($arg)*)
    )
);

/// Prints an alert-level message (level 1).
///
/// Use this level if action must be taken immediately.
///
/// Equivalent to the kernel's [`pr_alert`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_alert`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_alert
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_alert!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_alert (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::ALERT, false, $($arg)*)
    )
);

/// Prints an alert-level message (level 1) with ratelimit.
///
/// Use this level if action must be taken immediately.
///
/// Equivalent to the kernel's [`pr_alert_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_alert`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_alert
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_alert_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_alert_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::ALERT, false, $where, $($arg)*)
    )
);

/// Prints a critical-level message (level 2).
///
/// Use this level for critical conditions.
///
/// Equivalent to the kernel's [`pr_crit`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_crit`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_crit
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_crit!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_crit (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::CRIT, false, $($arg)*)
    )
);

/// Prints a critical-level message (level 2) with ratelimit.
///
/// Use this level for critical conditions.
///
/// Equivalent to the kernel's [`pr_crit_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_crit`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_crit
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_crit_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_crit_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::CRIT, false, $where, $($arg)*)
    )
);

/// Prints an error-level message (level 3).
///
/// Use this level for error conditions.
///
/// Equivalent to the kernel's [`pr_err`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_err`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_err
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_err!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_err (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::ERR, false, $($arg)*)
    )
);

/// Prints an error-level message (level 3) with ratelimit with ratelimit.
///
/// Use this level for error conditions.
///
/// Equivalent to the kernel's [`pr_err_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_err`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_err
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_err_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_err_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::ERR, false, $where, $($arg)*)
    )
);

/// Prints a warning-level message (level 4).
///
/// Use this level for warning conditions.
///
/// Equivalent to the kernel's [`pr_warn`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_warn`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_warn
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_warn!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_warn (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::WARNING, false, $($arg)*)
    )
);

/// Prints a warning-level message (level 4) with ratelimit.
///
/// Use this level for warning conditions.
///
/// Equivalent to the kernel's [`pr_warn_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_warn`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_warn
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_warn_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_warn_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::WARNING, false, $where, $($arg)*)
    )
);

/// Prints a notice-level message (level 5).
///
/// Use this level for normal but significant conditions.
///
/// Equivalent to the kernel's [`pr_notice`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_notice`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_notice
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_notice!("hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_notice (
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::NOTICE, false, $($arg)*)
    )
);

/// Prints a notice-level message (level 5) with ratelimit.
///
/// Use this level for normal but significant conditions.
///
/// Equivalent to the kernel's [`pr_notice_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_notice`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_notice
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_notice_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_notice_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::NOTICE, false, $where, $($arg)*)
    )
);

/// Prints an info-level message (level 6).
///
/// Use this level for informational messages.
///
/// Equivalent to the kernel's [`pr_info`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_info`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_info
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
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
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::INFO, false, $($arg)*)
    )
);

/// Prints an info-level message (level 6) with ratelimit.
///
/// Use this level for informational messages.
///
/// Equivalent to the kernel's [`pr_info_ratelimited`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_info`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_info
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// pr_info_ratelimited!("myfunc", "hello {}\n", "there");
/// ```
#[macro_export]
macro_rules! pr_info_ratelimited (
    ($where:literal, $($arg:tt)*) => (
        $crate::print_macro_ratelimited!($crate::print::format_strings::INFO, false, $where, $($arg)*)
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
    ($($arg:tt)*) => (
        if cfg!(debug_assertions) {
            $crate::print_macro!($crate::print::format_strings::DEBUG, false, $($arg)*)
        }
    )
);

/// Continues a previous log message in the same line.
///
/// Use only when continuing a previous `pr_*!` macro (e.g. [`pr_info!`]).
///
/// Equivalent to the kernel's [`pr_cont`] macro.
///
/// Mimics the interface of [`std::print!`]. See [`core::fmt`] and
/// [`alloc::format!`] for information about the formatting syntax.
///
/// [`pr_cont`]: https://www.kernel.org/doc/html/latest/core-api/printk-basics.html#c.pr_cont
/// [`std::print!`]: https://doc.rust-lang.org/std/macro.print.html
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
    ($($arg:tt)*) => (
        $crate::print_macro!($crate::print::format_strings::CONT, true, $($arg)*)
    )
);

/// Printk ratelimit control with a shared [`ratelimit_state`]. The state will be
/// shared with all this macro & kernel's [`printk_ratelimit`] callers.
///
/// Equivalent to the kernel's [`printk_ratelimit`] function.
///
/// Returns `true` if ratelimit control is enforced, otherwise `false`.
///
/// [`where`] parameter is a string literal that will be used as an
/// identifier when ratelimite is triggered. In C this is populated with
/// __function__.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::pr_cont;
///
/// if printk_ratelimit!("myfunc") {
///     pr_info!("hello");
///     pr_cont!(" {}\n", "there");
/// }
/// ```
#[cfg(not(testlib))]
#[macro_export]
macro_rules! printk_ratelimit (
    ($where:literal) => (
        // SAFETY: In any case the parameter should be a null-terminated string.
        unsafe { $crate::print::__printk_ratelimit($crate::c_str!($where).as_char_ptr()) } != 0
    )
);

/// Stub for doctests
#[cfg(testlib)]
#[macro_export]
macro_rules! printk_ratelimit (
    ($where:literal) => (
        true
    );
);
