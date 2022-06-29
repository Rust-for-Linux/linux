// SPDX-License-Identifier: GPL-2.0

//! Kernel page allocation and management.
//!
//! TODO: This module is a work in progress.

use crate::io_buffer::{IoBufferReader, IoBufferWriter};
use crate::{bindings, c_types, error::code::*, Result, PAGE_SIZE};
use core::{marker::PhantomData, ptr};

/// A set of physical pages.
///
/// `Pages` holds a reference to a set of pages of order `ORDER`. Having the order as a generic
/// const allows the struct to have the same size as a pointer.
///
/// # Invariants
///
/// The pointer `Pages::pages` is valid and points to 2^ORDER pages.
pub struct Pages<const ORDER: u32> {
    pub(crate) pages: *mut bindings::page,
}

impl<const ORDER: u32> Pages<ORDER> {
    /// Allocates a new set of contiguous pages.
    pub fn new() -> Result<Self> {
        // TODO: Consider whether we want to allow callers to specify flags.
        // SAFETY: This only allocates pages. We check that it succeeds in the next statement.
        let pages = unsafe {
            bindings::alloc_pages(
                bindings::GFP_KERNEL | bindings::__GFP_ZERO | bindings::__GFP_HIGHMEM,
                ORDER,
            )
        };
        if pages.is_null() {
            return Err(ENOMEM);
        }
        // INVARIANTS: We checked that the allocation above succeeded>
        Ok(Self { pages })
    }

    /// Copy to or from pages by using `copy_action` to do the data transfer.
    fn copy_with_pages<F>(&self, offset: usize, len: usize, mut copy_action: F) -> Result
    where
        F: FnMut(CopyCommand<'_>) -> Result,
    {
        let pages_in_buffer = (2_usize).pow(ORDER);
        let buffer_size = PAGE_SIZE * pages_in_buffer;
        let end = offset.checked_add(len).ok_or(EINVAL)?;
        if end > buffer_size {
            return Err(EINVAL);
        }

        let mut page_offset = offset % PAGE_SIZE;
        let page_index_start = offset / PAGE_SIZE;
        let x = (len / PAGE_SIZE) + if page_offset == 0 { 0 } else { 1 };
        let page_count = if len + page_offset <= x * PAGE_SIZE {
            x
        } else {
            x + 1
        };
        let page_index_end = page_index_start + page_count;
        let mut remaining = len;
        let mut written = 0;

        for i in page_index_start..page_index_end {
            assert!(remaining > 0);
            let mapping = self.kmap(i).ok_or(EINVAL)?;

            let size = core::cmp::min(remaining, PAGE_SIZE - page_offset);

            assert!(page_offset + size <= PAGE_SIZE);
            assert!(written + size <= len);
            copy_action(CopyCommand {
                page_mapping: &mapping,
                page_offset,
                written,
                size,
            })?;

            page_offset = 0;
            remaining -= size;
            written += size;
        }

        assert!(remaining == 0);
        assert!(written == len);
        Ok(())
    }

    /// Copies data from the given [`IoBufferReader`] into the pages.
    pub fn copy_into_page<R: IoBufferReader>(
        &self,
        reader: &mut R,
        offset: usize,
        len: usize,
    ) -> Result {
        self.copy_with_pages(offset, len, move |command| {
            // SAFETY: This is safe because `copy_with_pages` maintain `page_offset + size <= PAGE_SIZE`.
            unsafe {
                reader.read_raw(
                    (command.page_mapping.ptr as usize + command.page_offset) as _,
                    command.size,
                )
            }
        })
    }

    /// Copy data from pages to the given [`IoBufferWriter`].
    pub fn copy_from_page<W: IoBufferWriter>(
        &self,
        writer: &mut W,
        offset: usize,
        len: usize,
    ) -> Result {
        self.copy_with_pages(offset, len, move |command| {
            // SAFETY: This is safe because `copy_with_pages` maintain `page_offset + size <= PAGE_SIZE`.
            unsafe {
                writer.write_raw(
                    (command.page_mapping.ptr as usize + command.page_offset) as _,
                    command.size,
                )
            }
        })
    }

    /// Maps the pages and reads from them into the given buffer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the destination buffer is valid for the given length.
    /// Additionally, if the raw buffer is intended to be recast, they must ensure that the data
    /// can be safely cast; [`crate::io_buffer::ReadableFromBytes`] has more details about it.
    pub unsafe fn read(&self, dest: *mut u8, offset: usize, len: usize) -> Result {
        self.copy_with_pages(offset, len, move |command| {
            // SAFETY: This is safe because `copy_with_pages` maintain
            // `page_offset + size <= PAGE_SIZE` and `written + size < len`.
            unsafe {
                ptr::copy(
                    (command.page_mapping.ptr as *mut u8).add(command.page_offset),
                    dest.add(command.written),
                    command.size,
                )
            }
            Ok(())
        })
    }

    /// Maps the pages and writes into them from the given buffer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the buffer is valid for the given length. Additionally, if the
    /// page is (or will be) mapped by userspace, they must ensure that no kernel data is leaked
    /// through padding if it was cast from another type; [`crate::io_buffer::WritableToBytes`] has
    /// more details about it.
    pub unsafe fn write(&self, src: *const u8, offset: usize, len: usize) -> Result {
        self.copy_with_pages(offset, len, move |command| {
            // SAFETY: This is safe because `copy_with_pages` maintain
            // `page_offset + size <= PAGE_SIZE` and `written  + size < len`.
            unsafe {
                ptr::copy(
                    src.add(command.written),
                    (command.page_mapping.ptr as *mut u8).add(command.page_offset),
                    command.size,
                )
            }
            Ok(())
        })
    }

    /// Maps the page at index `index`.
    fn kmap(&self, index: usize) -> Option<PageMapping<'_>> {
        if index >= 1usize << ORDER {
            return None;
        }

        // SAFETY: We checked above that `index` is within range.
        let page = unsafe { self.pages.add(index) };

        // SAFETY: `page` is valid based on the checks above.
        let ptr = unsafe { bindings::kmap(page) };
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

struct CopyCommand<'a> {
    page_mapping: &'a PageMapping<'a>,
    page_offset: usize,
    written: usize,
    size: usize,
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
        unsafe { bindings::kunmap(self.page) };
    }
}
