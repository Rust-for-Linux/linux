// SPDX-License-Identifier: GPL-2.0

//! Allocator support.

use core::alloc::AllocError;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;

use crate::bindings;
use crate::static_assert;

pub(crate) struct KernelAllocator;

fn requires_header(layout: &Layout) -> bool {
    layout.align() > bindings::ARCH_KMALLOC_MINALIGN as usize
        && layout.size() > layout.align()
        && !layout.size().is_power_of_two()
}

// Header containing a pointer to the start of an allocated block.
// SAFETY: Size and alignment must be <= `bindings::ARCH_KALLOC_MINALIGN`.
#[repr(C)]
struct Header(*mut u8);

static_assert!(core::mem::size_of::<Header>() <= bindings::ARCH_KMALLOC_MINALIGN);
static_assert!(core::mem::align_of::<Header>() <= bindings::ARCH_KMALLOC_MINALIGN);

impl KernelAllocator {
    #[cfg(not(test))]
    #[cfg(not(testlib))]
    #[cold]
    #[inline(never)]
    fn allocate_with_header(
        &self,
        layout: Layout,
        flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError> {
        let alloc_size = layout.size() + layout.align();

        // SAFETY: `krealloc` will return a pointer to `alloc_size` memory block or a null pointer
        let block = unsafe { bindings::krealloc(ptr::null(), alloc_size, flags) as *mut u8 };
        if block.is_null() {
            return Err(AllocError);
        }

        // Create a correctly aligned pointer offset from the start of the allocated block,
        // and write a header before it.
        let offset = layout.align() - (block.addr() & (layout.align() - 1));

        // SAFETY: `bindings::ARCH_KALLOC_MINALIGN` <= `offset` <= `layout.align()` and the size
        // of the allocated block is `layout.align() + layout.size()`. `aligned` will thus be a
        // correctly aligned pointer inside the allocated block with at least `layout.size()`
        // bytes after it and at least `bindings::ARCH_KALLOC_MINALIGN` bytes of padding before
        // it.
        let aligned = unsafe { block.add(offset) };

        // SAFETY: Because the size and alignment of a header is <=
        // `bindings::ARCH_KALLOC_MINALIGN` and `aligned` is aligned to at least
        // `bindings::ARCH_KALLOC_MINALIGN` and has at least `bindings::ARCH_KALLOC_MINALIGN`
        // bytes of padding before it, it is safe to write a header directly before it.
        unsafe { ptr::write((aligned as *mut Header).offset(-1), Header(block)) };

        // SAFETY: The returned pointer does not point to the to the start of an allocated block,
        // but there is a header readable directly before it containing the location of the start
        // of the block. We checked that the pointer is not null above.
        return Ok(unsafe {
            NonNull::new_unchecked(core::slice::from_raw_parts_mut(aligned, layout.size()))
        });
    }

    #[cfg(not(test))]
    #[cfg(not(testlib))]
    pub(crate) fn allocate_with_flags(
        &self,
        mut layout: Layout,
        flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError> {
        if requires_header(&layout) {
            return self.allocate_with_header(layout, flags);
        }

        if layout.size() < layout.align() {
            layout = layout.pad_to_align();
        }

        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        let mem = unsafe { bindings::krealloc(ptr::null(), layout.size(), flags) as *mut u8 };
        if mem.is_null() {
            return Err(AllocError);
        }
        let mem = unsafe { core::slice::from_raw_parts_mut(mem, bindings::ksize(mem as _)) };
        // SAFETY: checked for non null above
        Ok(unsafe { NonNull::new_unchecked(mem) })
    }

    #[cfg(test)]
    #[cfg(testlib)]
    pub(crate) fn allocate_with_flags(
        &self,
        layout: Layout,
        _flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate(layout)
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.allocate_with_flags(layout, bindings::GFP_KERNEL) {
            Ok(x) => x.cast().as_ptr(),
            Err(_) => 0 as _,
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let block = if !requires_header(&layout) {
            ptr
        } else {
            // SAFETY: Safety requirements on this function requires that ptr was allocated with
            // this allocator and with the same layout passed to this funciton. Therefore ptr will
            // have a header structure placed at offset -1.
            unsafe { ptr::read((ptr as *mut Header).offset(-1)).0 }
        };

        unsafe {
            bindings::kfree(block as *const core::ffi::c_void);
        }
    }
}

#[global_allocator]
pub(crate) static ALLOCATOR: KernelAllocator = KernelAllocator;

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
