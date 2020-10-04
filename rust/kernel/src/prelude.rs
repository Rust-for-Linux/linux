// SPDX-License-Identifier: GPL-2.0

//! The `kernel` prelude

pub use alloc::{borrow::ToOwned, string::String};

pub use module::module;

pub use super::{eprintln, kprintln, println, KernelModule, KernelResult};
