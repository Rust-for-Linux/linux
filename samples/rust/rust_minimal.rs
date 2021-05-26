// SPDX-License-Identifier: GPL-2.0

//! Rust minimal sample

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::prelude::*;

module! {
    type: RustMinimal,
    name: "rust_minimal",
    author: "Rust for Linux Contributors",
    description: "Rust minimal sample",
    license: "GPL v2",
}

struct RustMinimal {
    message: String,
}

impl KernelModule for RustMinimal {
    fn init() -> Result<Self> {
        pr_info!("Rust minimal sample (init)\n");
        pr_info!("Am I built-in? {}\n", !cfg!(MODULE));

        Ok(RustMinimal {
            message: "on the heap!".to_owned(),
        })
    }
}

impl Drop for RustMinimal {
    fn drop(&mut self) {
        pr_info!("My message is {}\n", self.message);
        pr_info!("Rust minimal sample (exit)\n");
    }
}
