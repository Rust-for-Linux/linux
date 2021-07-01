// SPDX-License-Identifier: GPL-2.0

//! The `kernel` crate.
//!
//! This crate contains the kernel APIs that have been ported or wrapped for
//! usage by Rust code in the kernel and is shared by all of them.
//!
//! In other words, all the rest of the Rust code in the kernel (e.g. kernel
//! modules written in Rust) depends on [`core`], [`alloc`] and this crate.
//!
//! If you need a kernel C API that is not ported or wrapped yet here, then
//! do so first instead of bypassing this crate.

#![no_std]
#![feature(
    allocator_api,
    alloc_error_handler,
    associated_type_defaults,
    const_fn_trait_bound,
    const_mut_refs,
    const_panic,
    const_raw_ptr_deref,
    const_unreachable_unchecked,
    receiver_trait,
    try_reserve
)]
#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![deny(clippy::perf)]
#![deny(clippy::style)]
#![deny(rust_2018_idioms)]

// Ensure conditional compilation based on the kernel configuration works;
// otherwise we may silently break things like initcall handling.
#[cfg(not(CONFIG_RUST))]
compile_error!("Missing kernel configuration for conditional compilation");

#[cfg(not(test))]
#[cfg(not(testlib))]
mod allocator;

#[doc(hidden)]
pub mod bindings;

pub mod buffer;
pub mod c_types;
pub mod chrdev;
mod error;
pub mod file;
pub mod file_operations;
pub mod fs;
pub mod miscdev;
pub mod pages;
pub mod str;
pub mod traits;

pub mod linked_list;
mod raw_list;
pub mod rbtree;

#[doc(hidden)]
pub mod module_param;

mod build_assert;
pub mod prelude;
pub mod print;
pub mod random;
mod static_assert;
pub mod sync;

#[cfg(CONFIG_SYSCTL)]
pub mod sysctl;

pub mod io_buffer;
pub mod iov_iter;
pub mod of;
pub mod platdev;
mod types;
pub mod user_ptr;

#[doc(hidden)]
pub use build_error::build_error;

pub use crate::error::{Error, Result};
pub use crate::types::{Mode, ScopeGuard};

/// Page size defined in terms of the `PAGE_SHIFT` macro from C.
///
/// [`PAGE_SHIFT`]: ../../../include/asm-generic/page.h
pub const PAGE_SIZE: usize = 1 << bindings::PAGE_SHIFT;

/// Prefix to appear before log messages printed from within the kernel crate.
const __LOG_PREFIX: &[u8] = b"rust_kernel\0";

/// The top level entrypoint to implementing a kernel module.
///
/// For any teardown or cleanup operations, your type may implement [`Drop`].
pub trait KernelModule: Sized + Sync {
    /// Called at module initialization time.
    ///
    /// Use this method to perform whatever setup or registration your module
    /// should do.
    ///
    /// Equivalent to the `module_init` macro in the C API.
    fn init() -> Result<Self>;
}

/// Equivalent to `THIS_MODULE` in the C API.
///
/// C header: `include/linux/export.h`
pub struct ThisModule(*mut bindings::module);

// SAFETY: `THIS_MODULE` may be used from all threads within a module.
unsafe impl Sync for ThisModule {}

impl ThisModule {
    /// Creates a [`ThisModule`] given the `THIS_MODULE` pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be equal to the right `THIS_MODULE`.
    pub const unsafe fn from_ptr(ptr: *mut bindings::module) -> ThisModule {
        ThisModule(ptr)
    }

    /// Locks the module parameters to access them.
    ///
    /// Returns a [`KParamGuard`] that will release the lock when dropped.
    pub fn kernel_param_lock(&self) -> KParamGuard<'_> {
        // SAFETY: `kernel_param_lock` will check if the pointer is null and
        // use the built-in mutex in that case.
        #[cfg(CONFIG_SYSFS)]
        unsafe {
            bindings::kernel_param_lock(self.0)
        }

        KParamGuard { this_module: self }
    }
}

/// Scoped lock on the kernel parameters of [`ThisModule`].
///
/// Lock will be released when this struct is dropped.
pub struct KParamGuard<'a> {
    this_module: &'a ThisModule,
}

#[cfg(CONFIG_SYSFS)]
impl<'a> Drop for KParamGuard<'a> {
    fn drop(&mut self) {
        // SAFETY: `kernel_param_lock` will check if the pointer is null and
        // use the built-in mutex in that case. The existance of `self`
        // guarantees that the lock is held.
        unsafe { bindings::kernel_param_unlock(self.this_module.0) }
    }
}

/// Calculates the offset of a field from the beginning of the struct it belongs to.
///
/// # Example
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::offset_of;
/// struct Test {
///     a: u64,
///     b: u32,
/// }
///
/// fn test() {
///     // This prints `8`.
///     pr_info!("{}\n", offset_of!(Test, b));
/// }
/// ```
#[macro_export]
macro_rules! offset_of {
    ($type:ty, $($f:tt)*) => {{
        let tmp = core::mem::MaybeUninit::<$type>::uninit();
        let outer = tmp.as_ptr();
        // To avoid warnings when nesting `unsafe` blocks.
        #[allow(unused_unsafe)]
        // SAFETY: The pointer is valid and aligned, just not initialised; `addr_of` ensures that
        // we don't actually read from `outer` (which would be UB) nor create an intermediate
        // reference.
        let inner = unsafe { core::ptr::addr_of!((*outer).$($f)*) } as *const u8;
        // To avoid warnings when nesting `unsafe` blocks.
        #[allow(unused_unsafe)]
        // SAFETY: The two pointers are within the same allocation block.
        unsafe { inner.offset_from(outer as *const u8) }
    }}
}

/// Produces a pointer to an object from a pointer to one of its fields.
///
/// # Safety
///
/// Callers must ensure that the pointer to the field is in fact a pointer to the specified field,
/// as opposed to a pointer to another object of the same type.
///
/// # Example
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::container_of;
/// struct Test {
///     a: u64,
///     b: u32,
/// }
///
/// fn test() {
///     let test = Test { a: 10, b: 20 };
///     let b_ptr = &test.b;
///     let test_alias = unsafe { container_of!(b_ptr, Test, b) };
///     // This prints `true`.
///     pr_info!("{}\n", core::ptr::eq(&test, test_alias));
/// }
/// ```
#[macro_export]
macro_rules! container_of {
    ($ptr:expr, $type:ty, $($f:tt)*) => {{
        let offset = $crate::offset_of!($type, $($f)*);
        unsafe { ($ptr as *const _ as *const u8).offset(-offset) as *const $type }
    }}
}
