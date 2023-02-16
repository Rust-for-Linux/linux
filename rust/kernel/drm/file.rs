// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM File objects.
//!
//! C header: [`include/linux/drm/drm_file.h`](../../../../include/linux/drm/drm_file.h)

use crate::{bindings, drm, error::Result};
use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ops::Deref;

/// Trait that must be implemented by DRM drivers to represent a DRM File (a client instance).
pub trait DriverFile {
    /// The parent `Driver` implementation for this `DriverFile`.
    type Driver: drm::drv::Driver;

    /// Open a new file (called when a client opens the DRM device).
    fn open(device: &drm::device::Device<Self::Driver>) -> Result<Box<Self>>;
}

/// An open DRM File.
///
/// # Invariants
/// `raw` is a valid pointer to a `drm_file` struct.
#[repr(transparent)]
pub struct File<T: DriverFile> {
    raw: *mut bindings::drm_file,
    _p: PhantomData<T>,
}

pub(super) unsafe extern "C" fn open_callback<T: DriverFile>(
    raw_dev: *mut bindings::drm_device,
    raw_file: *mut bindings::drm_file,
) -> core::ffi::c_int {
    let drm = core::mem::ManuallyDrop::new(unsafe { drm::device::Device::from_raw(raw_dev) });
    // SAFETY: This reference won't escape this function
    let file = unsafe { &mut *raw_file };

    let inner = match T::open(&drm) {
        Err(e) => {
            return e.to_kernel_errno();
        }
        Ok(i) => i,
    };

    file.driver_priv = Box::into_raw(inner) as *mut _;

    0
}

pub(super) unsafe extern "C" fn postclose_callback<T: DriverFile>(
    _dev: *mut bindings::drm_device,
    raw_file: *mut bindings::drm_file,
) {
    // SAFETY: This reference won't escape this function
    let file = unsafe { &*raw_file };

    // Drop the DriverFile
    unsafe { Box::from_raw(file.driver_priv as *mut T) };
}

impl<T: DriverFile> File<T> {
    // Not intended to be called externally, except via declare_drm_ioctls!()
    #[doc(hidden)]
    pub unsafe fn from_raw(raw_file: *mut bindings::drm_file) -> File<T> {
        File {
            raw: raw_file,
            _p: PhantomData,
        }
    }

    #[allow(dead_code)]
    /// Return the raw pointer to the underlying `drm_file`.
    pub(super) fn raw(&self) -> *const bindings::drm_file {
        self.raw
    }

    /// Return an immutable reference to the raw `drm_file` structure.
    pub(super) fn file(&self) -> &bindings::drm_file {
        unsafe { &*self.raw }
    }
}

impl<T: DriverFile> Deref for File<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*(self.file().driver_priv as *const T) }
    }
}

impl<T: DriverFile> crate::private::Sealed for File<T> {}

/// Generic trait to allow users that don't care about driver specifics to accept any File<T>.
///
/// # Safety
/// Must only be implemented for File<T> and return the pointer, following the normal invariants
/// of that type.
pub unsafe trait GenericFile: crate::private::Sealed {
    /// Returns the raw const pointer to the `struct drm_file`
    fn raw(&self) -> *const bindings::drm_file;
    /// Returns the raw mut pointer to the `struct drm_file`
    fn raw_mut(&mut self) -> *mut bindings::drm_file;
}

unsafe impl<T: DriverFile> GenericFile for File<T> {
    fn raw(&self) -> *const bindings::drm_file {
        self.raw
    }
    fn raw_mut(&mut self) -> *mut bindings::drm_file {
        self.raw
    }
}
