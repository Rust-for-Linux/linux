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
        // SAFETY: FFI call.
        unsafe { bindings::krealloc(ptr::null(), layout.size(), bindings::GFP_KERNEL) as *mut u8 }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe {
            bindings::kfree(ptr as *const core::ffi::c_void);
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

// See <https://github.com/rust-lang/rust/pull/86844>.
#[no_mangle]
static __rust_no_alloc_shim_is_unstable: u8 = 0;
