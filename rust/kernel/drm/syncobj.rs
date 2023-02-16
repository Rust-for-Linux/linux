// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM Sync Objects
//!
//! C header: [`include/linux/drm/drm_syncobj.h`](../../../../include/linux/drm/drm_syncobj.h)

use crate::{bindings, dma_fence::*, drm, error::Result, prelude::*};

/// A DRM Sync Object
///
/// # Invariants
/// ptr is a valid pointer to a drm_syncobj and we own a reference to it.
pub struct SyncObj {
    ptr: *mut bindings::drm_syncobj,
}

impl SyncObj {
    /// Looks up a sync object by its handle for a given `File`.
    pub fn lookup_handle(file: &impl drm::file::GenericFile, handle: u32) -> Result<SyncObj> {
        // SAFETY: The arguments are all valid per the type invariants.
        let ptr = unsafe { bindings::drm_syncobj_find(file.raw() as *mut _, handle) };

        if ptr.is_null() {
            Err(ENOENT)
        } else {
            Ok(SyncObj { ptr })
        }
    }

    /// Returns the DMA fence associated with this sync object, if any.
    pub fn fence_get(&self) -> Option<Fence> {
        let fence = unsafe { bindings::drm_syncobj_fence_get(self.ptr) };
        if fence.is_null() {
            None
        } else {
            // SAFETY: The pointer is non-NULL and drm_syncobj_fence_get acquired an
            // additional reference.
            Some(unsafe { Fence::from_raw(fence) })
        }
    }

    /// Replaces the DMA fence with a new one, or removes it if fence is None.
    pub fn replace_fence(&self, fence: Option<&Fence>) {
        unsafe {
            bindings::drm_syncobj_replace_fence(
                self.ptr,
                fence.map_or(core::ptr::null_mut(), |a| a.raw()),
            )
        };
    }

    /// Adds a new timeline point to the syncobj.
    pub fn add_point(&self, chain: FenceChain, fence: &Fence, point: u64) {
        // SAFETY: All arguments should be valid per the respective type invariants.
        // This takes over the FenceChain ownership.
        unsafe { bindings::drm_syncobj_add_point(self.ptr, chain.into_raw(), fence.raw(), point) };
    }
}

impl Drop for SyncObj {
    fn drop(&mut self) {
        // SAFETY: We own a reference to this syncobj.
        unsafe { bindings::drm_syncobj_put(self.ptr) };
    }
}

impl Clone for SyncObj {
    fn clone(&self) -> Self {
        // SAFETY: `ptr` is valid per the type invariant and we own a reference to it.
        unsafe { bindings::drm_syncobj_get(self.ptr) };
        SyncObj { ptr: self.ptr }
    }
}

// SAFETY: drm_syncobj operations are internally locked.
unsafe impl Sync for SyncObj {}
unsafe impl Send for SyncObj {}
