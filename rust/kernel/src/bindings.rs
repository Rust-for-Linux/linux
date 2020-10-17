// SPDX-License-Identifier: GPL-2.0

#[allow(
    clippy::all,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    improper_ctypes
)]
mod bindings_raw {
    use crate::c_types;
    include!(env!("RUST_BINDGEN_FILE"));
}
pub use bindings_raw::*;

pub const GFP_KERNEL: gfp_t = BINDINGS_GFP_KERNEL;
