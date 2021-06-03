// SPDX-License-Identifier: GPL-2.0

//! Kernel page allocation and management.
//!
//! TODO: This module is a work in progress.

use crate::{
    bindings, c_types, error::from_kernel_int_result, io_buffer::IoBufferReader,
    user_ptr::UserSlicePtrReader, Error, Result, PAGE_SIZE,
};
use core::{marker::PhantomData, ptr};

extern "C" {
    #[allow(improper_ctypes)]
    fn rust_helper_alloc_pages(
        gfp_mask: bindings::gfp_t,
        order: c_types::c_uint,
    ) -> *mut bindings::page;

    #[allow(improper_ctypes)]
    fn rust_helper_kmap(page: *mut bindings::page) -> *mut c_types::c_void;

    #[allow(improper_ctypes)]
    fn rust_helper_kunmap(page: *mut bindings::page);
}

/// A set of physical pages.
///
/// `Pages` holds a reference to a set of pages of order `ORDER`. Having the order as a generic
/// const allows the struct to have the same size as a pointer.
///
/// # Invariants
///
/// The pointer [`Pages::pages`] is valid and points to 2^ORDER pages.
pub struct Pages<const ORDER: u32> {
    pages: *mut bindings::page,
}

impl<const ORDER: u32> Pages<ORDER> {
    /// Allocates a new set of contiguous pages.
    pub fn new() -> Result<Self> {
        // TODO: Consider whether we want to allow callers to specify flags.
        // SAFETY: This only allocates pages. We check that it succeeds in the next statement.
        let pages = unsafe {
            rust_helper_alloc_pages(
                bindings::GFP_KERNEL | bindings::__GFP_ZERO | bindings::__GFP_HIGHMEM,
                ORDER,
            )
        };
        if pages.is_null() {
            return Err(Error::ENOMEM);
        }
        // INVARIANTS: We checked that the allocation above succeeded>
        Ok(Self { pages })
    }

    /// Maps a single page at the given address in the given VM area.
    ///
    /// This is only meant to be used by pages of order 0.
    pub fn insert_page(&self, vma: &mut bindings::vm_area_struct, address: usize) -> Result {
        if ORDER != 0 {
            return Err(Error::EINVAL);
        }

        // SAFETY: We check above that the allocation is of order 0. The range of `address` is
        // already checked by `vm_insert_page`.
        let ret = unsafe { bindings::vm_insert_page(vma, address as _, self.pages) };
        // SAFETY: `bindings::vm_insert_page()` returns zero on success,
        // or a valid negative `errno` on error.
        unsafe { from_kernel_int_result(ret) }
    }

    /// Copies data from the given [`UserSlicePtrReader`] into the pages.
    pub fn copy_into_page(
        &self,
        reader: &mut UserSlicePtrReader,
        offset: usize,
        len: usize,
    ) -> Result {
        // TODO: For now this only works on the first page.
        let end = offset.checked_add(len).ok_or(Error::EINVAL)?;
        if end > PAGE_SIZE {
            return Err(Error::EINVAL);
        }

        let mapping = self.kmap(0).ok_or(Error::EINVAL)?;

        // SAFETY: We ensured that the buffer was valid with the check above.
        unsafe { reader.read_raw((mapping.ptr as usize + offset) as _, len) }?;
        Ok(())
    }

    /// Maps the pages and reads from them into the given buffer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the destination buffer is valid for the given length.
    /// Additionally, if the raw buffer is intended to be recast, they must ensure that the data
    /// can be safely cast; [`crate::io_buffer::ReadableFromBytes`] has more details about it.
    pub unsafe fn read(&self, dest: *mut u8, offset: usize, len: usize) -> Result {
        // TODO: For now this only works on the first page.
        let end = offset.checked_add(len).ok_or(Error::EINVAL)?;
        if end > PAGE_SIZE {
            return Err(Error::EINVAL);
        }

        let mapping = self.kmap(0).ok_or(Error::EINVAL)?;
        ptr::copy((mapping.ptr as *mut u8).add(offset), dest, len);
        Ok(())
    }

    /// Maps the pages and writes into them from the given bufer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the buffer is valid for the given length. Additionally, if the
    /// page is (or will be) mapped by userspace, they must ensure that no kernel data is leaked
    /// through padding if it was cast from another type; [`crate::io_buffer::WritableToBytes`] has
    /// more details about it.
    pub unsafe fn write(&self, src: *const u8, offset: usize, len: usize) -> Result {
        // TODO: For now this only works on the first page.
        let end = offset.checked_add(len).ok_or(Error::EINVAL)?;
        if end > PAGE_SIZE {
            return Err(Error::EINVAL);
        }

        let mapping = self.kmap(0).ok_or(Error::EINVAL)?;
        ptr::copy(src, (mapping.ptr as *mut u8).add(offset), len);
        Ok(())
    }

    /// Maps the page at index `index`.
    fn kmap(&self, index: usize) -> Option<PageMapping<'_>> {
        if index >= 1usize << ORDER {
            return None;
        }

        // SAFETY: We checked above that `index` is within range.
        let page = unsafe { self.pages.add(index) };

        // SAFETY: `page` is valid based on the checks above.
        let ptr = unsafe { rust_helper_kmap(page) };
        if ptr.is_null() {
            return None;
        }

        Some(PageMapping {
            page,
            ptr,
            _phantom: PhantomData,
        })
    }
}

impl<const ORDER: u32> Drop for Pages<ORDER> {
    fn drop(&mut self) {
        // SAFETY: By the type invariants, we know the pages are allocated with the given order.
        unsafe { bindings::__free_pages(self.pages, ORDER) };
    }
}

struct PageMapping<'a> {
    page: *mut bindings::page,
    ptr: *mut c_types::c_void,
    _phantom: PhantomData<&'a i32>,
}

impl Drop for PageMapping<'_> {
    fn drop(&mut self) {
        // SAFETY: An instance of `PageMapping` is created only when `kmap` succeeded for the given
        // page, so it is safe to unmap it here.
        unsafe { rust_helper_kunmap(self.page) };
    }
}
