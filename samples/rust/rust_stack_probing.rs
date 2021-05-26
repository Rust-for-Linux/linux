// SPDX-License-Identifier: GPL-2.0

//! Rust stack probing sample

#![no_std]
#![feature(allocator_api, global_asm)]
#![feature(test)]

use kernel::prelude::*;

module! {
    type: RustStackProbing,
    name: "rust_stack_probing",
    author: "Rust for Linux Contributors",
    description: "Rust stack probing sample",
    license: "GPL v2",
}

struct RustStackProbing;

impl KernelModule for RustStackProbing {
    fn init() -> Result<Self> {
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
