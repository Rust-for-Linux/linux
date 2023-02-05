// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM File objects.
//!
//! C header: [`include/linux/drm/drm_file.h`](srctree/include/linux/drm/drm_file.h)

use crate::{bindings, drm, error::Result, prelude::*};
use core::marker::PhantomData;
use core::pin::Pin;

/// Trait that must be implemented by DRM drivers to represent a DRM File (a client instance).
pub trait DriverFile {
    /// The parent `Driver` implementation for this `DriverFile`.
    type Driver: drm::drv::Driver;

    /// Open a new file (called when a client opens the DRM device).
    fn open(device: &drm::device::Device<Self::Driver>) -> Result<Pin<KBox<Self>>>;
}

/// An open DRM File.
///
/// # Invariants
/// `raw` is a valid pointer to an open `drm_file` struct.
#[repr(transparent)]
pub struct File<T: DriverFile> {
    raw: *mut bindings::drm_file,
    _p: PhantomData<T>,
}

#[allow(clippy::missing_safety_doc)]
/// The open callback of a `struct drm_file`.
pub(super) unsafe extern "C" fn open_callback<T: DriverFile>(
    raw_dev: *mut bindings::drm_device,
    raw_file: *mut bindings::drm_file,
) -> core::ffi::c_int {
    // SAFETY: A callback from `struct drm_driver::open` gurantees that `raw_dev` is valid
    // pointer to a `sturct drm_device`.
    let drm = unsafe { drm::device::Device::borrow(raw_dev) };
    // SAFETY: This reference won't escape this function
    let file = unsafe { &mut *raw_file };

    let inner = match T::open(drm) {
        Err(e) => {
            return e.to_errno();
        }
        Ok(i) => i,
    };

    // SAFETY: This pointer is treated as pinned, and the Drop guarantee is upheld below.
    file.driver_priv = KBox::into_raw(unsafe { Pin::into_inner_unchecked(inner) }) as *mut _;

    0
}

#[allow(clippy::missing_safety_doc)]
/// The postclose callback of a `struct drm_file`.
pub(super) unsafe extern "C" fn postclose_callback<T: DriverFile>(
    _raw_dev: *mut bindings::drm_device,
    raw_file: *mut bindings::drm_file,
) {
    // SAFETY: This reference won't escape this function
    let file = unsafe { &*raw_file };

    // SAFETY: `file.driver_priv` has been created in `open_callback` through `KBox::into_raw`.
    unsafe {
        let _ = KBox::from_raw(file.driver_priv as *mut T);
    };
}

impl<T: DriverFile> File<T> {
    #[doc(hidden)]
    /// Not intended to be called externally, except via declare_drm_ioctls!()
    ///
    /// # Safety
    ///
    /// `raw_file` must be a valid pointer to an open `struct drm_file`, opened through `T::open`.
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
        // SAFETY: `self.raw` is a valid pointer to a `struct drm_file`.
        unsafe { &*self.raw }
    }

    /// Return a pinned reference to the driver file structure.
    pub fn inner(&self) -> Pin<&T> {
        // SAFETY: By the type invariant the pointer `self.raw` points to a valid and opened
        // `struct drm_file`, hence `self.raw.driver_priv` has been properly initialized by
        // `open_callback`.
        unsafe { Pin::new_unchecked(&*(self.file().driver_priv as *const T)) }
    }
}

impl<T: DriverFile> crate::private::Sealed for File<T> {}

/// Generic trait to allow users that don't care about driver specifics to accept any `File<T>`.
///
/// # Safety
///
/// Must only be implemented for `File<T>` and return the pointer, following the normal invariants
/// of that type.
pub unsafe trait GenericFile: crate::private::Sealed {
    /// Returns the raw const pointer to the `struct drm_file`
    fn raw(&self) -> *const bindings::drm_file;
    /// Returns the raw mut pointer to the `struct drm_file`
    fn raw_mut(&mut self) -> *mut bindings::drm_file;
}

// SAFETY: Implementation for `File<T>`, holding up its type invariants.
unsafe impl<T: DriverFile> GenericFile for File<T> {
    fn raw(&self) -> *const bindings::drm_file {
        self.raw
    }
    fn raw_mut(&mut self) -> *mut bindings::drm_file {
        self.raw
    }
}
