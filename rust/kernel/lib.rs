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
#![feature(allocator_api)]
#![feature(associated_type_defaults)]
#![feature(coerce_unsized)]
#![feature(const_mut_refs)]
#![feature(const_refs_to_cell)]
#![feature(const_trait_impl)]
#![feature(c_size_t)]
#![feature(dispatch_from_dyn)]
#![feature(doc_cfg)]
#![feature(duration_constants)]
#![feature(ptr_metadata)]
#![feature(receiver_trait)]
#![feature(unsize)]

// Ensure conditional compilation based on the kernel configuration works;
// otherwise we may silently break things like initcall handling.
#[cfg(not(CONFIG_RUST))]
compile_error!("Missing kernel configuration for conditional compilation");

// allow proc-macros to refer to `::kernel`.
extern crate self as kernel;

#[cfg(not(test))]
#[cfg(not(testlib))]
mod allocator;

#[doc(hidden)]
pub use bindings;

pub use macros;

#[cfg(CONFIG_ARM_AMBA)]
pub mod amba;
pub mod chrdev;
#[cfg(CONFIG_COMMON_CLK)]
pub mod clk;
pub mod cred;
pub mod delay;
pub mod device;
pub mod driver;
pub mod error;
pub mod file;
pub mod fs;
pub mod gpio;
pub mod hwrng;
pub mod irq;
pub mod kasync;
pub mod miscdev;
pub mod mm;
#[cfg(CONFIG_NET)]
pub mod net;
pub mod pages;
pub mod power;
pub mod revocable;
pub mod security;
pub mod str;
pub mod task;
pub mod workqueue;

pub mod linked_list;
mod raw_list;
pub mod rbtree;
pub mod unsafe_list;

#[doc(hidden)]
pub mod module_param;

mod build_assert;
pub mod prelude;
pub mod print;
pub mod random;
mod static_assert;
#[doc(hidden)]
pub mod std_vendor;
pub mod sync;

#[cfg(any(CONFIG_SYSCTL, doc))]
#[doc(cfg(CONFIG_SYSCTL))]
pub mod sysctl;

pub mod io_buffer;
#[cfg(CONFIG_HAS_IOMEM)]
pub mod io_mem;
pub mod iov_iter;
pub mod of;
pub mod platform;
mod types;
pub mod user_ptr;

#[cfg(CONFIG_KUNIT)]
pub mod kunit;

#[doc(hidden)]
pub use build_error::build_error;

pub use crate::error::{to_result, Error, Result};
pub use crate::types::{
    bit, bits_iter, ARef, AlwaysRefCounted, Bit, Bool, Either, Either::Left, Either::Right, False,
    Mode, Opaque, PointerWrapper, ScopeGuard, True,
};

use core::marker::PhantomData;

/// Page size defined in terms of the `PAGE_SHIFT` macro from C.
///
/// [`PAGE_SHIFT`]: ../../../include/asm-generic/page.h
pub const PAGE_SIZE: usize = 1 << bindings::PAGE_SHIFT;

/// Prefix to appear before log messages printed from within the kernel crate.
const __LOG_PREFIX: &[u8] = b"rust_kernel\0";

/// The top level entrypoint to implementing a kernel module.
///
/// For any teardown or cleanup operations, your type may implement [`Drop`].
pub trait Module: Sized + Sync {
    /// Called at module initialization time.
    ///
    /// Use this method to perform whatever setup or registration your module
    /// should do.
    ///
    /// Equivalent to the `module_init` macro in the C API.
    fn init(name: &'static str::CStr, module: &'static ThisModule) -> Result<Self>;
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

        KParamGuard {
            #[cfg(CONFIG_SYSFS)]
            this_module: self,
            phantom: PhantomData,
        }
    }
}

/// Scoped lock on the kernel parameters of [`ThisModule`].
///
/// Lock will be released when this struct is dropped.
pub struct KParamGuard<'a> {
    #[cfg(CONFIG_SYSFS)]
    this_module: &'a ThisModule,
    phantom: PhantomData<&'a ()>,
}

#[cfg(CONFIG_SYSFS)]
impl<'a> Drop for KParamGuard<'a> {
    fn drop(&mut self) {
        // SAFETY: `kernel_param_lock` will check if the pointer is null and
        // use the built-in mutex in that case. The existence of `self`
        // guarantees that the lock is held.
        unsafe { bindings::kernel_param_unlock(self.this_module.0) }
    }
}

/// Calculates the offset of a field from the beginning of the struct it belongs to.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::offset_of;
/// struct Test {
///     a: u64,
///     b: u32,
/// }
///
/// assert_eq!(offset_of!(Test, b), 8);
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
/// as opposed to a pointer to another object of the same type. If this condition is not met,
/// any dereference of the resulting pointer is UB.
///
/// # Examples
///
/// ```
/// # use kernel::container_of;
/// struct Test {
///     a: u64,
///     b: u32,
/// }
///
/// let test = Test { a: 10, b: 20 };
/// let b_ptr = &test.b;
/// let test_alias = container_of!(b_ptr, Test, b);
/// assert!(core::ptr::eq(&test, test_alias));
/// ```
#[macro_export]
macro_rules! container_of {
    ($ptr:expr, $type:ty, $($f:tt)*) => {{
        let ptr = $ptr as *const _ as *const u8;
        let offset = $crate::offset_of!($type, $($f)*);
        ptr.wrapping_offset(-offset) as *const $type
    }}
}

#[cfg(not(any(testlib, test)))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    pr_emerg!("{}\n", info);
    // SAFETY: FFI call.
    unsafe { bindings::BUG() };
    // Bindgen currently does not recognize `__noreturn` so `BUG` returns `()`
    // instead of `!`. See <https://github.com/rust-lang/rust-bindgen/issues/2094>.
    loop {}
}
