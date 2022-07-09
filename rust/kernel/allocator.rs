// SPDX-License-Identifier: GPL-2.0

//! Allocator support.

use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use crate::bindings;

#[derive(Copy, Clone)]
pub struct KernelAllocator<const A: bindings::gfp_t>;

unsafe impl GlobalAlloc for KernelAllocator<{ bindings::GFP_KERNEL }> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        unsafe { bindings::krealloc(ptr::null(), layout.size(), bindings::GFP_KERNEL) as *mut u8 }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe {
            bindings::kfree(ptr as *const core::ffi::c_void);
        }
    }
}

unsafe impl<const A: bindings::gfp_t> Allocator for KernelAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        let mem = unsafe { bindings::krealloc(ptr::null(), layout.size(), A) as *mut u8 };
        if mem.is_null() {
            return Err(AllocError);
        }
        let mem = unsafe { core::slice::from_raw_parts_mut(mem, layout.size()) };

        // Safety: checked for non null abpve
        Ok(unsafe { NonNull::new_unchecked(mem) })
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _layout: Layout) {
        unsafe {
            bindings::kfree(ptr.as_ptr() as *const core::ffi::c_void);
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator<{ bindings::GFP_KERNEL }> = KernelAllocator;

/// Allocator using [`bindings::GFP_ATOMIC`].
///
/// # Example
///
/// ```
/// use kernel::ALLOCATOR_ATOMIC;
/// use alloc::vec::Vec;
///
/// let mut vec = Vec::new_in(ALLOCATOR_ATOMIC);
/// vec.try_push(1).unwrap();
/// ```
pub static ALLOCATOR_ATOMIC: KernelAllocator<{ bindings::GFP_ATOMIC }> = KernelAllocator;

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
