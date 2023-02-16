// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM device.
//!
//! C header: [`include/linux/drm/drm_device.h`](../../../../include/linux/drm/drm_device.h)

use crate::{bindings, device, drm, types::ForeignOwnable};
use core::marker::PhantomData;

/// Represents a reference to a DRM device. The device is reference-counted and is guaranteed to
/// not be dropped while this object is alive.
pub struct Device<T: drm::drv::Driver> {
    // Type invariant: ptr must be a valid and initialized drm_device,
    // and this value must either own a reference to it or the caller
    // must ensure that it is never dropped if the reference is borrowed.
    pub(super) ptr: *mut bindings::drm_device,
    _p: PhantomData<T>,
}

impl<T: drm::drv::Driver> Device<T> {
    // Not intended to be called externally, except via declare_drm_ioctls!()
    #[doc(hidden)]
    pub unsafe fn from_raw(raw: *mut bindings::drm_device) -> Device<T> {
        Device {
            ptr: raw,
            _p: PhantomData,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn raw(&self) -> *const bindings::drm_device {
        self.ptr
    }

    pub(crate) fn raw_mut(&mut self) -> *mut bindings::drm_device {
        self.ptr
    }

    /// Returns a borrowed reference to the user data associated with this Device.
    pub fn data(&self) -> <T::Data as ForeignOwnable>::Borrowed<'_> {
        unsafe { T::Data::borrow((*self.ptr).dev_private) }
    }
}

impl<T: drm::drv::Driver> Drop for Device<T> {
    fn drop(&mut self) {
        // SAFETY: By the type invariants, we know that `self` owns a reference, so it is safe to
        // relinquish it now.
        unsafe { bindings::drm_dev_put(self.ptr) };
    }
}

impl<T: drm::drv::Driver> Clone for Device<T> {
    fn clone(&self) -> Self {
        // SAFETY: We get a new reference and then create a new owning object from the raw pointer
        unsafe {
            bindings::drm_dev_get(self.ptr);
            Device::from_raw(self.ptr)
        }
    }
}

// SAFETY: `Device` only holds a pointer to a C device, which is safe to be used from any thread.
unsafe impl<T: drm::drv::Driver> Send for Device<T> {}

// SAFETY: `Device` only holds a pointer to a C device, references to which are safe to be used
// from any thread.
unsafe impl<T: drm::drv::Driver> Sync for Device<T> {}

// Make drm::Device work for dev_info!() and friends
unsafe impl<T: drm::drv::Driver> device::RawDevice for Device<T> {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: ptr must be valid per the type invariant
        unsafe { (*self.ptr).dev }
    }
}
