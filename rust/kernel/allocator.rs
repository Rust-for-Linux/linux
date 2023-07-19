// SPDX-License-Identifier: GPL-2.0

//! Allocator support.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use crate::bindings;

struct KernelAllocator;

// SAFETY: this implementation meets `GlobalAlloc` safety invariants:
// - `kalloc` does not unwind
// - `kalloc` has no stricter safety requirements than those of `GlobalAlloc` itself
// - Allocating has no further side effects
unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        // SAFETY: `krealloc` is a FFI call with no invariants to meet
        unsafe { bindings::krealloc(ptr::null(), layout.size(), bindings::GFP_KERNEL).cast() }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: `dealloc` has the invariant that `ptr` was allocated by this
        // allocator. `kfree` has no additional invariants.
        unsafe { bindings::kfree(ptr.cast()) };
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

// `rustc` only generates these for some crate types. Even then, we would need
// to extract the object file that has them from the archive. For the moment,
// let's generate them ourselves instead.
//
// Note that `#[no_mangle]` implies exported too, nowadays.
#[no_mangle]
fn __rust_alloc(size: usize, _align: usize) -> *mut u8 {
    // SAFETY: `krealloc` is a FFI call with no invariants to meet
    unsafe { bindings::krealloc(core::ptr::null(), size, bindings::GFP_KERNEL).cast() }
}

#[no_mangle]
fn __rust_dealloc(ptr: *mut u8, _size: usize, _align: usize) {
    // SAFETY: `ptr` only ever comes from `krealloc` via `__rust_alloc`
    unsafe { bindings::kfree(ptr.cast()) };
}

#[no_mangle]
fn __rust_realloc(ptr: *mut u8, _old_size: usize, _align: usize, new_size: usize) -> *mut u8 {
    // SAFETY: `ptr` only ever comes from `krealloc` via `__rust_alloc`
    unsafe { bindings::krealloc(ptr.cast(), new_size, bindings::GFP_KERNEL).cast() }
}

#[no_mangle]
fn __rust_alloc_zeroed(size: usize, _align: usize) -> *mut u8 {
    // SAFETY: `krealloc` can handle zero-sized allocations with `__GFP_ZERO`
    unsafe {
        bindings::krealloc(
            core::ptr::null(),
            size,
            bindings::GFP_KERNEL | bindings::__GFP_ZERO,
        )
        .cast()
    }
}
