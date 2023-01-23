// SPDX-License-Identifier: GPL-2.0

//! Bindings.
//!
//! Imports the generated bindings by `bindgen`.
//!
//! This crate may not be directly used. If you need a kernel C API that is
//! not ported or wrapped in the `kernel` crate, then do so first instead of
//! using this crate.

#![no_std]
// See <https://github.com/rust-lang/rust-bindgen/issues/1651>.
#![cfg_attr(test, allow(deref_nullptr))]
#![cfg_attr(test, allow(unaligned_references))]
#![cfg_attr(test, allow(unsafe_op_in_unsafe_fn))]
#![allow(
    clippy::all,
    missing_docs,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    improper_ctypes,
    unreachable_pub,
    unsafe_op_in_unsafe_fn
)]

mod bindings_raw {
    // Use glob import here to expose all helpers.
    // Symbols defined within the module will take precedence to the glob import.
    pub use super::bindings_helper::*;
    include!(concat!(
        env!("OBJTREE"),
        "/rust/bindings/bindings_generated.rs"
    ));
}

// When both a directly exposed symbol and a helper exists for the same function,
// the directly exposed symbol is preferred and the helper becomes dead code, so
// ignore the warning here.
#[allow(dead_code)]
mod bindings_helper {
    // Import the generated bindings for types.
    use super::bindings_raw::*;
    include!(concat!(
        env!("OBJTREE"),
        "/rust/bindings/bindings_helpers_generated.rs"
    ));
}

pub use bindings_raw::*;

pub const GFP_ATOMIC: gfp_t = BINDINGS_GFP_ATOMIC;
pub const GFP_KERNEL: gfp_t = BINDINGS_GFP_KERNEL;
pub const GFP_NOWAIT: gfp_t = BINDINGS_GFP_NOWAIT;
pub const GFP_NOIO: gfp_t = BINDINGS_GFP_NOIO;
pub const GFP_NOFS: gfp_t = BINDINGS_GFP_NOFS;
pub const GFP_USER: gfp_t = BINDINGS_GFP_USER;
pub const __GFP_NOFAIL: gfp_t = ___GFP_NOFAIL;
pub const __GFP_NORETRY: gfp_t = ___GFP_NORETRY;
pub const __GFP_NOWARN: gfp_t = ___GFP_NOWARN;
pub const __GFP_COMP: gfp_t = ___GFP_COMP;
pub const __GFP_ZERO: gfp_t = ___GFP_ZERO;
pub const __GFP_HIGHMEM: gfp_t = ___GFP_HIGHMEM;

pub const MAX_LFS_FILESIZE: loff_t = BINDINGS_MAX_LFS_FILESIZE;
