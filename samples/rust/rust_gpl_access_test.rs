// SPDX-License-Identifier: GPL-2.0

//! Rust GPL access test.
//!
//! This module should fail to pass `modpost`.

use kernel::prelude::*;

// This module is licensed as GPL as the SPDX license identifier above mentions.
// However, the value of the `license` field below is different in order to
// ensure that `modpost` catches innocent mistakes when a non-GPL compatible
// module tries to use GPL-only symbols in Rust.
module! {
    type: RustGplAccessTest,
    name: "rust_gpl_access_test",
    author: "Rust for Linux Contributors",
    description: "Rust GPL access test",
    license: "Happy Little Accidents License",
}

/// Function in a non-GPL compatible module that uses a Rust symbol exported
/// using `EXPORT_SYMBOL_GPL()`.
///
/// This is __not__ OK, and should be detected by `modpost`.
fn happy_little_accident() {
    pr_info!("...and in our world, we can do anything that we want to do here\n");
}

/// Function in a non-GPL compatible module that uses a C symbol exported
/// using `EXPORT_SYMBOL_GPL()`.
///
/// This is __not__ OK, and should be detected by `modpost`.
fn this_little_rascal_here() {
    extern "C" {
        fn kernel_halt();
    }

    // SAFETY: FFI call with no safety preconditions.
    unsafe {
        kernel_halt();
    }
}

struct RustGplAccessTest;

impl kernel::Module for RustGplAccessTest {
    fn init(_name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        happy_little_accident();
        this_little_rascal_here();

        Ok(RustGplAccessTest)
    }
}
