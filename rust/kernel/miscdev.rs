// SPDX-License-Identifier: GPL-2.0

//! Miscellaneous devices.
//!
//! C header: [`include/linux/miscdevice.h`](../../../../include/linux/miscdevice.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/driver-api/misc_devices.html>

use crate::bindings;
use crate::error::{code::*, Error, Result};
use crate::file;
use crate::{device, str::CStr, str::CString, ThisModule};
use alloc::boxed::Box;
use core::marker::PhantomPinned;
use core::{fmt, mem::MaybeUninit, pin::Pin};

/// Options which can be used to configure how a misc device is registered.
///
/// # Examples
///
/// ```
/// # use kernel::{c_str, device::RawDevice, file, miscdev, prelude::*};
/// fn example(
///     reg: Pin<&mut miscdev::Registration<impl file::Operations<OpenData = ()>>>,
///     parent: &dyn RawDevice,
/// ) -> Result {
///     miscdev::Options::new()
///         .mode(0o600)
///         .minor(10)
///         .parent(parent)
///         .register(reg, fmt!("sample"), ())
/// }
/// ```
#[derive(Default)]
pub struct Options<'a> {
    minor: Option<i32>,
    mode: Option<u16>,
    parent: Option<&'a dyn device::RawDevice>,
}

impl<'a> Options<'a> {
    /// Creates new [`Options`] instance with the required fields.
    pub const fn new() -> Self {
        Self {
            minor: None,
            mode: None,
            parent: None,
        }
    }

    /// Sets the minor device number.
    pub const fn minor(&mut self, v: i32) -> &mut Self {
        self.minor = Some(v);
        self
    }

    /// Sets the device mode.
    ///
    /// This is usually an octal number and describes who can perform read/write/execute operations
    /// on the device.
    pub const fn mode(&mut self, m: u16) -> &mut Self {
        self.mode = Some(m);
        self
    }

    /// Sets the device parent.
    pub const fn parent(&mut self, p: &'a dyn device::RawDevice) -> &mut Self {
        self.parent = Some(p);
        self
    }

    /// Registers a misc device using the configured options.
    pub fn register<T: file::Operations>(
        &self,
        reg: Pin<&mut Registration<T>>,
        name: fmt::Arguments<'_>,
        open_data: T::OpenData,
    ) -> Result {
        reg.register_with_options(name, open_data, self)
    }

    /// Allocates a new registration of a misc device and completes the registration with the
    /// configured options.
    pub fn register_new<T: file::Operations>(
        &self,
        name: fmt::Arguments<'_>,
        open_data: T::OpenData,
    ) -> Result<Pin<Box<Registration<T>>>> {
        let mut r = Pin::from(Box::try_new(Registration::new())?);
        self.register(r.as_mut(), name, open_data)?;
        Ok(r)
    }
}

/// A registration of a miscellaneous device.
///
/// # Invariants
///
/// `Context` is always initialised when `registered` is `true`, and not initialised otherwise.
pub struct Registration<T: file::Operations> {
    registered: bool,
    mdev: bindings::miscdevice,
    name: Option<CString>,
    _pin: PhantomPinned,

    /// Context initialised on construction and made available to all file instances on
    /// [`file::Operations::open`].
    open_data: MaybeUninit<T::OpenData>,
}

impl<T: file::Operations> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new() -> Self {
        // INVARIANT: `registered` is `false` and `open_data` is not initialised.
        Self {
            registered: false,
            mdev: bindings::miscdevice::default(),
            name: None,
            _pin: PhantomPinned,
            open_data: MaybeUninit::uninit(),
        }
    }

    /// Registers a miscellaneous device.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned(name: fmt::Arguments<'_>, open_data: T::OpenData) -> Result<Pin<Box<Self>>> {
        Options::new().register_new(name, open_data)
    }

    /// Registers a miscellaneous device with the rest of the kernel.
    ///
    /// It must be pinned because the memory block that represents the registration is
    /// self-referential.
    pub fn register(
        self: Pin<&mut Self>,
        name: fmt::Arguments<'_>,
        open_data: T::OpenData,
    ) -> Result {
        Options::new().register(self, name, open_data)
    }

    /// Registers a miscellaneous device with the rest of the kernel. Additional optional settings
    /// are provided via the `opts` parameter.
    ///
    /// It must be pinned because the memory block that represents the registration is
    /// self-referential.
    pub fn register_with_options(
        self: Pin<&mut Self>,
        name: fmt::Arguments<'_>,
        open_data: T::OpenData,
        opts: &Options<'_>,
    ) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            // Already registered.
            return Err(EINVAL);
        }

        let name = CString::try_from_fmt(name)?;

        // SAFETY: The adapter is compatible with `misc_register`.
        this.mdev.fops = unsafe { file::OperationsVtable::<Self, T>::build() };
        this.mdev.name = name.as_char_ptr();
        this.mdev.minor = opts.minor.unwrap_or(bindings::MISC_DYNAMIC_MINOR as i32);
        this.mdev.mode = opts.mode.unwrap_or(0);
        this.mdev.parent = opts
            .parent
            .map_or(core::ptr::null_mut(), |p| p.raw_device());

        // We write to `open_data` here because as soon as `misc_register` succeeds, the file can be
        // opened, so we need `open_data` configured ahead of time.
        //
        // INVARIANT: `registered` is set to `true`, but `open_data` is also initialised.
        this.registered = true;
        this.open_data.write(open_data);

        let ret = unsafe { bindings::misc_register(&mut this.mdev) };
        if ret < 0 {
            // INVARIANT: `registered` is set back to `false` and the `open_data` is destructued.
            this.registered = false;
            // SAFETY: `open_data` was initialised a few lines above.
            unsafe { this.open_data.assume_init_drop() };
            return Err(Error::from_kernel_errno(ret));
        }

        this.name = Some(name);

        Ok(())
    }
}

impl<T: file::Operations> Default for Registration<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: file::Operations> file::OpenAdapter<T::OpenData> for Registration<T> {
    unsafe fn convert(
        _inode: *mut bindings::inode,
        file: *mut bindings::file,
    ) -> *const T::OpenData {
        // SAFETY: The caller must guarantee that `file` is valid.
        let reg = crate::container_of!(unsafe { (*file).private_data }, Self, mdev);

        // SAFETY: This function is only called while the misc device is still registered, so the
        // registration must be valid. Additionally, the type invariants guarantee that while the
        // miscdev is registered, `open_data` is initialised.
        unsafe { (*reg).open_data.as_ptr() }
    }
}

// SAFETY: The only method is `register()`, which requires a (pinned) mutable `Registration`, so it
// is safe to pass `&Registration` to multiple threads because it offers no interior mutability.
unsafe impl<T: file::Operations> Sync for Registration<T> {}

// SAFETY: All functions work from any thread. So as long as the `Registration::open_data` is
// `Send`, so is `Registration<T>`.
unsafe impl<T: file::Operations> Send for Registration<T> where T::OpenData: Send {}

impl<T: file::Operations> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        if self.registered {
            // SAFETY: `registered` being `true` indicates that a previous call to  `misc_register`
            // succeeded.
            unsafe { bindings::misc_deregister(&mut self.mdev) };

            // SAFETY: The type invariant guarantees that `open_data` is initialised when
            // `registered` is `true`.
            unsafe { self.open_data.assume_init_drop() };
        }
    }
}

/// Kernel module that exposes a single miscdev device implemented by `T`.
pub struct Module<T: file::Operations<OpenData = ()>> {
    _dev: Pin<Box<Registration<T>>>,
}

impl<T: file::Operations<OpenData = ()>> crate::Module for Module<T> {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        Ok(Self {
            _dev: Registration::new_pinned(crate::fmt!("{name}"), ())?,
        })
    }
}

/// Declares a kernel module that exposes a single misc device.
///
/// The `type` argument should be a type which implements the [`FileOpener`] trait. Also accepts
/// various forms of kernel metadata.
///
/// C header: [`include/linux/moduleparam.h`](../../../include/linux/moduleparam.h)
///
/// [`FileOpener`]: ../kernel/file_operations/trait.FileOpener.html
///
/// # Examples
///
/// ```ignore
/// use kernel::prelude::*;
///
/// module_misc_device! {
///     type: MyFile,
///     name: b"my_miscdev_kernel_module",
///     author: b"Rust for Linux Contributors",
///     description: b"My very own misc device kernel module!",
///     license: b"GPL",
/// }
///
/// #[derive(Default)]
/// struct MyFile;
///
/// #[vtable]
/// impl kernel::file::Operations for MyFile {}
/// ```
#[macro_export]
macro_rules! module_misc_device {
    (type: $type:ty, $($f:tt)*) => {
        type ModuleType = kernel::miscdev::Module<$type>;
        module! {
            type: ModuleType,
            $($f)*
        }
    }
}
