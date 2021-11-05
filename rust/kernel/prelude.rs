// SPDX-License-Identifier: GPL-2.0

//! The `kernel` prelude.
//!
//! These are the most common items used by Rust code in the kernel,
//! intended to be imported by all Rust code, for convenience.
//!
//! # Examples
//!
//! ```
//! use kernel::prelude::*;
//! ```

pub use core::pin::Pin;

pub use alloc::{boxed::Box, string::String, vec::Vec};

pub use macros::{module, module_misc_device};

pub use super::build_assert;

pub use super::{
    dbg, pr_alert, pr_alert_ratelimited, pr_crit, pr_crit_ratelimited, pr_debug, pr_emerg,
    pr_emerg_ratelimited, pr_err, pr_err_ratelimited, pr_info, pr_info_ratelimited, pr_notice,
    pr_notice_ratelimited, pr_warn, pr_warn_ratelimited, printk_ratelimit,
};

pub use super::static_assert;

pub use super::{Error, KernelModule, Result};
