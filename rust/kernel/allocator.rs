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

// `rustc` only generates these for some crate types. Even then, we would need
// to extract the object file that has them from the archive. For the moment,
// let's generate them ourselves instead.
//
// Note that `#[no_mangle]` implies exported too, nowadays.
#[no_mangle]
fn __rust_alloc(size: usize, _align: usize) -> *mut u8 {
    unsafe { bindings::krealloc(core::ptr::null(), size, bindings::GFP_KERNEL) as *mut u8 }
}

#[no_mangle]
fn __rust_dealloc(ptr: *mut u8, _size: usize, _align: usize) {
    unsafe { bindings::kfree(ptr as *const core::ffi::c_void) };
}

#[no_mangle]
fn __rust_realloc(ptr: *mut u8, _old_size: usize, _align: usize, new_size: usize) -> *mut u8 {
    unsafe {
        bindings::krealloc(
            ptr as *const core::ffi::c_void,
            new_size,
            bindings::GFP_KERNEL,
        ) as *mut u8
    }
}

#[no_mangle]
fn __rust_alloc_zeroed(size: usize, _align: usize) -> *mut u8 {
    unsafe {
        bindings::krealloc(
            core::ptr::null(),
            size,
            bindings::GFP_KERNEL | bindings::__GFP_ZERO,
        ) as *mut u8
    }
}
