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

pub use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

pub use macros::{module, module_misc_device};

pub use super::build_assert;

pub use super::{pr_alert, pr_crit, pr_debug, pr_emerg, pr_err, pr_info, pr_notice, pr_warn};

pub use super::static_assert;

pub use super::{Error, KernelModule, Result};

pub use crate::traits::TryPin;

// Default log prefix to none so crates without `module!` can use printk macros.
#[doc(hidden)]
pub const __LOG_PREFIX: () = ();
