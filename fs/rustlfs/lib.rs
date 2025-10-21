// SPDX-License-Identifier: GPL-2.0

//! Log-based filesystem written in Rust

use kernel::prelude::*;

module! {
    type: RustLFS,
    name: "rustlfs",
    authors: ["ls4121@columbia.edu", "kfb2117@columbia.edu"],
    description: "Log-based file system in Rust",
    license: "GPL",
}

struct RustLFS;

impl kernel::Module for RustLFS {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("RustLFS loaded\n");
        Ok(RustLFS)
    }
}

impl Drop for RustLFS {
    fn drop(&mut self) {
        pr_info!("RustLFS unloaded\n")
    }
}
