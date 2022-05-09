// SPDX-License-Identifier: GPL-2.0

//! Rust stack probing sample.

use kernel::prelude::*;

module! {
    type: RustStackProbing,
    name: b"rust_stack_probing",
    author: b"Rust for Linux Contributors",
    description: b"Rust stack probing sample",
    license: b"GPL v2",
}

struct RustStackProbing;

impl kernel::Module for RustStackProbing {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust stack probing sample (init)\n");

        // Including this large variable on the stack will trigger
        // stack probing on the supported archs.
        // This will verify that stack probing does not lead to
        // any errors if we need to link `__rust_probestack`.
        let x: [u64; 514] = core::hint::black_box([5; 514]);
        pr_info!("Large array has length: {}\n", x.len());

        Ok(RustStackProbing)
    }
}

impl Drop for RustStackProbing {
    fn drop(&mut self) {
        pr_info!("Rust stack probing sample (exit)\n");
    }
}
