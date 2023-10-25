// SPDX-License-Identifier: GPL-2.0

//! Kernel memory caches (kmem_cache).
//!
//! C headers: [`include/linux/slab.h`](../../include/linux/slab.h)

use crate::error::{code::*, Result};
use crate::{bindings, str::CStr};
use core::ptr;

/// A kernel memory cache.
///
/// This isn't ready to be made public yet because it only provides functionality useful for the
/// allocation of inodes in file systems.
pub(crate) struct MemCache {
    ptr: ptr::NonNull<bindings::kmem_cache>,
}

impl MemCache {
    /// Allocates a new `kmem_cache` for type `T`.
    ///
    /// `init` is called by the C code when entries are allocated.
    pub(crate) fn try_new<T>(
        name: &'static CStr,
        init: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
    ) -> Result<Self> {
        // SAFETY: `name` is static, so always valid.
        let ptr = ptr::NonNull::new(unsafe {
            bindings::kmem_cache_create(
                name.as_char_ptr(),
                core::mem::size_of::<T>().try_into()?,
                core::mem::align_of::<T>().try_into()?,
                bindings::SLAB_RECLAIM_ACCOUNT | bindings::SLAB_MEM_SPREAD | bindings::SLAB_ACCOUNT,
                init,
            )
        })
        .ok_or(ENOMEM)?;

        Ok(Self { ptr })
    }

    /// Returns the pointer to the `kmem_cache` instance, or null if it's `None`.
    ///
    /// This is a helper for functions like `alloc_inode_sb` where the cache is optional.
    pub(crate) fn ptr(c: &Option<Self>) -> *mut bindings::kmem_cache {
        match c {
            Some(m) => m.ptr.as_ptr(),
            None => ptr::null_mut(),
        }
    }
}

impl Drop for MemCache {
    fn drop(&mut self) {
        // SAFETY: Just an FFI call with no additional safety requirements.
        unsafe { bindings::rcu_barrier() };

        // SAFETY: `ptr` was previously returned by successful call to `kmem_cache_create`, so it's
        // ok to destroy it here.
        unsafe { bindings::kmem_cache_destroy(self.ptr.as_ptr()) };
    }
}
