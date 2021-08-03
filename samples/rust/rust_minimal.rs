// SPDX-License-Identifier: GPL-2.0

//! Rust minimal sample

#![no_std]
#![feature(allocator_api, global_asm)]

use alloc::try_format;
use kernel::prelude::*;

module! {
    type: RustMinimal,
    name: b"rust_minimal",
    author: b"Rust for Linux Contributors",
    description: b"Rust minimal sample",
    license: b"GPL v2",
}

struct RustMinimal {
    message: String,
}

impl KernelModule for RustMinimal {
    fn init() -> Result<Self> {
        pr_info!("Rust minimal sample (init)\n");
        pr_info!("Am I built-in? {}\n", !cfg!(MODULE));

        Ok(RustMinimal {
            message: try_format!("on the {}!", "heap")?,
        })
    }
}

impl Drop for RustMinimal {
    fn drop(&mut self) {
        pr_info!("My message is {}\n", self.message);
        pr_info!("Rust minimal sample (exit)\n");
    }
}
