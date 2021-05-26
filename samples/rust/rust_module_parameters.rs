// SPDX-License-Identifier: GPL-2.0

//! Rust module parameters sample

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::prelude::*;

module! {
    type: RustModuleParameters,
    name: "rust_module_parameters",
    author: "Rust for Linux Contributors",
    description: "Rust module parameters sample",
    license: "GPL v2",
    params: {
        my_bool: bool {
            default: true,
            permissions: 0,
            description: "Example of bool",
        },
        my_i32: i32 {
            default: 42,
            permissions: 0o644,
            description: "Example of i32",
        },
        my_str: str {
            default: "default str val",
            permissions: 0o644,
            description: "Example of a string param",
        },
        my_usize: usize {
            default: 42,
            permissions: 0o644,
            description: "Example of usize",
        },
        my_array: ArrayParam<i32, 3> {
            default: [0, 1],
            permissions: 0,
            description: "Example of array",
        },
    },
}

struct RustModuleParameters;

impl KernelModule for RustModuleParameters {
    fn init() -> Result<Self> {
        pr_info!("Rust module parameters sample (init)\n");

        {
            let lock = THIS_MODULE.kernel_param_lock();
            pr_info!("Parameters:\n");
            pr_info!("  my_bool:    {}\n", my_bool.read());
            pr_info!("  my_i32:     {}\n", my_i32.read(&lock));
            pr_info!(
                "  my_str:     {}\n",
                core::str::from_utf8(my_str.read(&lock))?
            );
            pr_info!("  my_usize:   {}\n", my_usize.read(&lock));
            pr_info!("  my_array:   {:?}\n", my_array.read());
        }

        Ok(RustModuleParameters)
    }
}

impl Drop for RustModuleParameters {
    fn drop(&mut self) {
        pr_info!("Rust module parameters sample (exit)\n");
    }
}
