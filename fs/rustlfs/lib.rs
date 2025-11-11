// SPDX-License-Identifier: GPL-2.0

//! Log-based filesystem written in Rust

mod defs;
mod dir;
mod inode;
mod sb;
use kernel::{c_str, fs, module_fs, prelude::*, str::CStr};

module_fs! {
    type: RustLFS,
    name: "rustlfs",
    authors: ["ls4121@columbia.edu", "kfb2117@columbia.edu"],
    description: "Log-based file system in Rust",
    license: "GPL",
}

struct RustLFS;

impl fs::Type for RustLFS {
    const NAME: &'static CStr = c_str!("rustlfs");
}
