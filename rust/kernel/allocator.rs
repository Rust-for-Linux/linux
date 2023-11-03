// SPDX-License-Identifier: GPL-2.0

//! Allocator support.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use crate::bindings;

struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        // SAFETY: calling C, layout is non zero as per function
        unsafe { bindings::krealloc(ptr::null(), layout.size(), bindings::GFP_KERNEL) as *mut u8 }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: calling C, ptr is valid and from `krealloc` or `kmalloc`.
        unsafe {
            bindings::kfree(ptr as *const core::ffi::c_void);
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        // SAFETY: calling C, layout is non zero as per function
        unsafe {
            bindings::krealloc(
                ptr::null(),
                layout.size(),
                bindings::GFP_KERNEL | bindings::__GFP_ZERO,
            ) as *mut u8
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, _layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: calling C, new_size is non zero as per function and prt is valid.
        unsafe {
            bindings::krealloc(
                ptr as *const core::ffi::c_void,
                new_size,
                bindings::GFP_KERNEL,
            ) as *mut u8
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

// See <https://github.com/rust-lang/rust/pull/86844>.
#[no_mangle]
static __rust_no_alloc_shim_is_unstable: u8 = 0;
