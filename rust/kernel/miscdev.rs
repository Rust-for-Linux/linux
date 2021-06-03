// SPDX-License-Identifier: GPL-2.0

//! Miscellaneous devices.
//!
//! C header: [`include/linux/miscdevice.h`](../../../../include/linux/miscdevice.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/driver-api/misc_devices.html>

use crate::bindings;
use crate::error::{from_kernel_int_result, Error, Result};
use crate::file_operations::{FileOpenAdapter, FileOpener, FileOperationsVtable};
use crate::str::CStr;
use alloc::boxed::Box;
use core::marker::PhantomPinned;
use core::pin::Pin;

/// A registration of a miscellaneous device.
pub struct Registration<T: Sync = ()> {
    registered: bool,
    mdev: bindings::miscdevice,
    _pin: PhantomPinned,

    /// Context initialised on construction and made available to all file instances on
    /// [`FileOpener::open`].
    pub context: T,
}

impl<T: Sync> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new(context: T) -> Self {
        Self {
            registered: false,
            mdev: bindings::miscdevice::default(),
            _pin: PhantomPinned,
            context,
        }
    }

    /// Registers a miscellaneous device.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned<F: FileOpener<T>>(
        name: &'static CStr,
        minor: Option<i32>,
        context: T,
    ) -> Result<Pin<Box<Self>>> {
        let mut r = Pin::from(Box::try_new(Self::new(context))?);
        r.as_mut().register::<F>(name, minor)?;
        Ok(r)
    }

    /// Registers a miscellaneous device with the rest of the kernel.
    ///
    /// It must be pinned because the memory block that represents the registration is
    /// self-referential. If a minor is not given, the kernel allocates a new one if possible.
    pub fn register<F: FileOpener<T>>(
        self: Pin<&mut Self>,
        name: &'static CStr,
        minor: Option<i32>,
    ) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            // Already registered.
            return Err(Error::EINVAL);
        }

        // SAFETY: The adapter is compatible with `misc_register`.
        this.mdev.fops = unsafe { FileOperationsVtable::<Self, F>::build() };
        this.mdev.name = name.as_char_ptr();
        this.mdev.minor = minor.unwrap_or(bindings::MISC_DYNAMIC_MINOR as i32);

        let ret = unsafe { bindings::misc_register(&mut this.mdev) };
        // SAFETY: `bindings::alloc_chrdev_region()` returns zero on
        // success, or a valid negative `errno` on error.
        unsafe {
            from_kernel_int_result(ret)?;
        }
        this.registered = true;
        Ok(())
    }
}

impl<T: Sync> FileOpenAdapter for Registration<T> {
    type Arg = T;

    unsafe fn convert(_inode: *mut bindings::inode, file: *mut bindings::file) -> *const Self::Arg {
        let reg = crate::container_of!((*file).private_data, Self, mdev);
        &(*reg).context
    }
}

// SAFETY: The only method is `register()`, which requires a (pinned) mutable `Registration`, so it
// is safe to pass `&Registration` to multiple threads because it offers no interior mutability,
// except maybe through `Registration::context`, but it is itself `Sync`.
unsafe impl<T: Sync> Sync for Registration<T> {}

// SAFETY: All functions work from any thread. So as long as the `Registration::context` is
// `Send`, so is `Registration<T>`. `T` needs to be `Sync` because it's a requirement of
// `Registration<T>`.
unsafe impl<T: Send + Sync> Send for Registration<T> {}

impl<T: Sync> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        if self.registered {
            unsafe { bindings::misc_deregister(&mut self.mdev) }
        }
    }
}
