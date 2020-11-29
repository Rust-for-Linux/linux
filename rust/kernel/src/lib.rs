// SPDX-License-Identifier: GPL-2.0

//! The `kernel` crate

#![no_std]
#![feature(allocator_api, alloc_error_handler, negative_impls)]

// Ensure conditional compilation based on the kernel configuration works;
// otherwise we may silently break things like initcall handling.
#[cfg(not(CONFIG_HAS_RUST))]
compile_error!("Missing kernel configuration for conditional compilation");

extern crate alloc;

use core::panic::PanicInfo;

mod allocator;
pub mod bindings;
pub mod c_types;
pub mod chrdev;
mod error;
pub mod file_operations;
pub mod prelude;
pub mod printk;
pub mod random;

pub mod sync;

#[cfg(CONFIG_SYSCTL)]
pub mod sysctl;

mod types;
pub mod user_ptr;

pub use crate::error::{Error, KernelResult};
pub use crate::types::{CStr, Mode};

/// KernelModule is the top level entrypoint to implementing a kernel module. Your kernel module
/// should implement the `init` method on it, which maps to the `module_init` macro in Linux C API.
/// You can use this method to do whatever setup or registration your module should do. For any
/// teardown or cleanup operations, your type may implement [`Drop`].
///
/// [`Drop`]: https://doc.rust-lang.org/stable/core/ops/trait.Drop.html
pub trait KernelModule: Sized + Sync {
    fn init() -> KernelResult<Self>;
}

extern "C" {
    fn rust_helper_BUG() -> !;
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        rust_helper_BUG();
    }
}

#[global_allocator]
static ALLOCATOR: allocator::KernelAllocator = allocator::KernelAllocator;
