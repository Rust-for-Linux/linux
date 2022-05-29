// SPDX-License-Identifier: GPL-2.0

//! Kernel async functionality.

pub mod executor;
#[cfg(CONFIG_NET)]
pub mod net;
