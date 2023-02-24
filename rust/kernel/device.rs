// SPDX-License-Identifier: GPL-2.0

//! Generic devices that are part of the kernel's driver model.
//!
//! C header: [`include/linux/device.h`](../../../../include/linux/device.h)

use crate::bindings;

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
}
