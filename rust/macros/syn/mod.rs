// SPDX-License-Identifier: MIT or Apache-2.0

//! Shared parsing functions for use in procedural macros.
//!
//! These code are from [syn 1.0.72](https://github.com/dtolnay/syn).

#![allow(dead_code)]

pub mod buffer;
mod lit;

pub use lit::{Lit, LitByteStr, LitStr};
