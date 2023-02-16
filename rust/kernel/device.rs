// SPDX-License-Identifier: GPL-2.0

//! Generic devices that are part of the kernel's driver model.
//!
//! C header: [`include/linux/device.h`](../../../../include/linux/device.h)

use crate::{bindings, str::CStr};

/// A raw device.
///
/// # Safety
///
/// Implementers must ensure that the `*mut device` returned by [`RawDevice::raw_device`] is
/// related to `self`, that is, actions on it will affect `self`. For example, if one calls
/// `get_device`, then the refcount on the device represented by `self` will be incremented.
///
/// Additionally, implementers must ensure that the device is never renamed. Commit a5462516aa99
/// ("driver-core: document restrictions on device_rename()") has details on why `device_rename`
/// should not be used.
pub unsafe trait RawDevice {
    /// Returns the raw `struct device` related to `self`.
    fn raw_device(&self) -> *mut bindings::device;

    /// Returns the name of the device.
    fn name(&self) -> &CStr {
        let ptr = self.raw_device();

        // SAFETY: `ptr` is valid because `self` keeps it alive.
        let name = unsafe { bindings::dev_name(ptr) };

        // SAFETY: The name of the device remains valid while it is alive (because the device is
        // never renamed, per the safety requirement of this trait). This is guaranteed to be the
        // case because the reference to `self` outlives the one of the returned `CStr` (enforced
        // by the compiler because of their lifetimes).
        unsafe { CStr::from_char_ptr(name) }
    }
}

/// A ref-counted device.
///
/// # Invariants
///
/// `ptr` is valid, non-null, and has a non-zero reference count. One of the references is owned by
/// `self`, and will be decremented when `self` is dropped.
pub struct Device {
    pub(crate) ptr: *mut bindings::device,
}

// SAFETY: `Device` only holds a pointer to a C device, which is safe to be used from any thread.
unsafe impl Send for Device {}

// SAFETY: `Device` only holds a pointer to a C device, references to which are safe to be used
// from any thread.
unsafe impl Sync for Device {}

impl Device {
    /// Creates a new device instance.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `ptr` is valid, non-null, and has a non-zero reference count.
    pub unsafe fn new(ptr: *mut bindings::device) -> Self {
        // SAFETY: By the safety requirements, ptr is valid and its refcounted will be incremented.
        unsafe { bindings::get_device(ptr) };
        // INVARIANT: The safety requirements satisfy all but one invariant, which is that `self`
        // owns a reference. This is satisfied by the call to `get_device` above.
        Self { ptr }
    }

    /// Creates a new device instance from an existing [`RawDevice`] instance.
    pub fn from_dev(dev: &dyn RawDevice) -> Self {
        // SAFETY: The requirements are satisfied by the existence of `RawDevice` and its safety
        // requirements.
        unsafe { Self::new(dev.raw_device()) }
    }
}

// SAFETY: The device returned by `raw_device` is the one for which we hold a reference.
unsafe impl RawDevice for Device {
    fn raw_device(&self) -> *mut bindings::device {
        self.ptr
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: By the type invariants, we know that `self` owns a reference, so it is safe to
        // relinquish it now.
        unsafe { bindings::put_device(self.ptr) };
    }
}

impl Clone for Device {
    fn clone(&self) -> Self {
        Device::from_dev(self)
    }
}
