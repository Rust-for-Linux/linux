// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM driver core.
//!
//! C header: [`include/linux/drm/drm_drv.h`](../../../../include/linux/drm/drm_drv.h)

use crate::{
    bindings, device, drm,
    error::code::*,
    error::from_kernel_err_ptr,
    error::{Error, Result},
    prelude::*,
    private::Sealed,
    str::CStr,
    types::ForeignOwnable,
    ThisModule,
};
use core::{
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
};
use macros::vtable;

/// Driver use the GEM memory manager. This should be set for all modern drivers.
pub const FEAT_GEM: u32 = bindings::drm_driver_feature_DRIVER_GEM;
/// Driver supports mode setting interfaces (KMS).
pub const FEAT_MODESET: u32 = bindings::drm_driver_feature_DRIVER_MODESET;
/// Driver supports dedicated render nodes.
pub const FEAT_RENDER: u32 = bindings::drm_driver_feature_DRIVER_RENDER;
/// Driver supports the full atomic modesetting userspace API.
///
/// Drivers which only use atomic internally, but do not support the full userspace API (e.g. not
/// all properties converted to atomic, or multi-plane updates are not guaranteed to be tear-free)
/// should not set this flag.
pub const FEAT_ATOMIC: u32 = bindings::drm_driver_feature_DRIVER_ATOMIC;
/// Driver supports DRM sync objects for explicit synchronization of command submission.
pub const FEAT_SYNCOBJ: u32 = bindings::drm_driver_feature_DRIVER_SYNCOBJ;
/// Driver supports the timeline flavor of DRM sync objects for explicit synchronization of command
/// submission.
pub const FEAT_SYNCOBJ_TIMELINE: u32 = bindings::drm_driver_feature_DRIVER_SYNCOBJ_TIMELINE;

/// Information data for a DRM Driver.
pub struct DriverInfo {
    /// Driver major version.
    pub major: i32,
    /// Driver minor version.
    pub minor: i32,
    /// Driver patchlevel version.
    pub patchlevel: i32,
    /// Driver name.
    pub name: &'static CStr,
    /// Driver description.
    pub desc: &'static CStr,
    /// Driver date.
    pub date: &'static CStr,
}

/// Internal memory management operation set, normally created by memory managers (e.g. GEM).
///
/// See `kernel::drm::gem` and `kernel::drm::gem::shmem`.
pub struct AllocOps {
    pub(crate) gem_create_object: Option<
        unsafe extern "C" fn(
            dev: *mut bindings::drm_device,
            size: usize,
        ) -> *mut bindings::drm_gem_object,
    >,
    pub(crate) prime_handle_to_fd: Option<
        unsafe extern "C" fn(
            dev: *mut bindings::drm_device,
            file_priv: *mut bindings::drm_file,
            handle: u32,
            flags: u32,
            prime_fd: *mut core::ffi::c_int,
        ) -> core::ffi::c_int,
    >,
    pub(crate) prime_fd_to_handle: Option<
        unsafe extern "C" fn(
            dev: *mut bindings::drm_device,
            file_priv: *mut bindings::drm_file,
            prime_fd: core::ffi::c_int,
            handle: *mut u32,
        ) -> core::ffi::c_int,
    >,
    pub(crate) gem_prime_import: Option<
        unsafe extern "C" fn(
            dev: *mut bindings::drm_device,
            dma_buf: *mut bindings::dma_buf,
        ) -> *mut bindings::drm_gem_object,
    >,
    pub(crate) gem_prime_import_sg_table: Option<
        unsafe extern "C" fn(
            dev: *mut bindings::drm_device,
            attach: *mut bindings::dma_buf_attachment,
            sgt: *mut bindings::sg_table,
        ) -> *mut bindings::drm_gem_object,
    >,
    pub(crate) gem_prime_mmap: Option<
        unsafe extern "C" fn(
            obj: *mut bindings::drm_gem_object,
            vma: *mut bindings::vm_area_struct,
        ) -> core::ffi::c_int,
    >,
    pub(crate) dumb_create: Option<
        unsafe extern "C" fn(
            file_priv: *mut bindings::drm_file,
            dev: *mut bindings::drm_device,
            args: *mut bindings::drm_mode_create_dumb,
        ) -> core::ffi::c_int,
    >,
    pub(crate) dumb_map_offset: Option<
        unsafe extern "C" fn(
            file_priv: *mut bindings::drm_file,
            dev: *mut bindings::drm_device,
            handle: u32,
            offset: *mut u64,
        ) -> core::ffi::c_int,
    >,
    pub(crate) dumb_destroy: Option<
        unsafe extern "C" fn(
            file_priv: *mut bindings::drm_file,
            dev: *mut bindings::drm_device,
            handle: u32,
        ) -> core::ffi::c_int,
    >,
}

/// Trait for memory manager implementations. Implemented internally.
pub trait AllocImpl: Sealed + drm::gem::IntoGEMObject {
    /// The C callback operations for this memory manager.
    const ALLOC_OPS: AllocOps;
}

/// A DRM driver implementation.
#[vtable]
pub trait Driver {
    /// Context data associated with the DRM driver
    ///
    /// Determines the type of the context data passed to each of the methods of the trait.
    type Data: ForeignOwnable + Sync + Send;

    /// The type used to manage memory for this driver.
    ///
    /// Should be either `drm::gem::Object<T>` or `drm::gem::shmem::Object<T>`.
    type Object: AllocImpl;

    /// The type used to represent a DRM File (client)
    type File: drm::file::DriverFile;

    /// Driver metadata
    const INFO: DriverInfo;

    /// Feature flags
    const FEATURES: u32;

    /// IOCTL list. See `kernel::drm::ioctl::declare_drm_ioctls!{}`.
    const IOCTLS: &'static [drm::ioctl::DrmIoctlDescriptor];
}

/// A registration of a DRM device
///
/// # Invariants:
///
/// drm is always a valid pointer to an allocated drm_device
pub struct Registration<T: Driver> {
    drm: drm::device::Device<T>,
    registered: bool,
    fops: bindings::file_operations,
    vtable: Pin<Box<bindings::drm_driver>>,
    _p: PhantomData<T>,
    _pin: PhantomPinned,
}

#[cfg(CONFIG_DRM_LEGACY)]
macro_rules! drm_legacy_fields {
    ( $($field:ident: $val:expr),* $(,)? ) => {
        bindings::drm_driver {
            $( $field: $val ),*,
            firstopen: None,
            preclose: None,
            dma_ioctl: None,
            dma_quiescent: None,
            context_dtor: None,
            irq_handler: None,
            irq_preinstall: None,
            irq_postinstall: None,
            irq_uninstall: None,
            get_vblank_counter: None,
            enable_vblank: None,
            disable_vblank: None,
            dev_priv_size: 0,
        }
    }
}

#[cfg(not(CONFIG_DRM_LEGACY))]
macro_rules! drm_legacy_fields {
    ( $($field:ident: $val:expr),* $(,)? ) => {
        bindings::drm_driver {
            $( $field: $val ),*
        }
    }
}

/// Registers a DRM device with the rest of the kernel.
///
/// It automatically picks up THIS_MODULE.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! drm_device_register {
    ($reg:expr, $data:expr, $flags:expr $(,)?) => {{
        $crate::drm::drv::Registration::register($reg, $data, $flags, &crate::THIS_MODULE)
    }};
}

impl<T: Driver> Registration<T> {
    const VTABLE: bindings::drm_driver = drm_legacy_fields! {
        load: None,
        open: Some(drm::file::open_callback::<T::File>),
        postclose: Some(drm::file::postclose_callback::<T::File>),
        lastclose: None,
        unload: None,
        release: None,
        master_set: None,
        master_drop: None,
        debugfs_init: None,
        gem_create_object: T::Object::ALLOC_OPS.gem_create_object,
        prime_handle_to_fd: T::Object::ALLOC_OPS.prime_handle_to_fd,
        prime_fd_to_handle: T::Object::ALLOC_OPS.prime_fd_to_handle,
        gem_prime_import: T::Object::ALLOC_OPS.gem_prime_import,
        gem_prime_import_sg_table: T::Object::ALLOC_OPS.gem_prime_import_sg_table,
        gem_prime_mmap: T::Object::ALLOC_OPS.gem_prime_mmap,
        dumb_create: T::Object::ALLOC_OPS.dumb_create,
        dumb_map_offset: T::Object::ALLOC_OPS.dumb_map_offset,
        dumb_destroy: T::Object::ALLOC_OPS.dumb_destroy,

        major: T::INFO.major,
        minor: T::INFO.minor,
        patchlevel: T::INFO.patchlevel,
        name: T::INFO.name.as_char_ptr() as *mut _,
        desc: T::INFO.desc.as_char_ptr() as *mut _,
        date: T::INFO.date.as_char_ptr() as *mut _,

        driver_features: T::FEATURES,
        ioctls: T::IOCTLS.as_ptr(),
        num_ioctls: T::IOCTLS.len() as i32,
        fops: core::ptr::null_mut(),
    };

    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new(parent: &dyn device::RawDevice) -> Result<Self> {
        let vtable = Pin::new(Box::try_new(Self::VTABLE)?);
        let raw_drm = unsafe { bindings::drm_dev_alloc(&*vtable, parent.raw_device()) };
        let raw_drm = from_kernel_err_ptr(raw_drm)?;

        // The reference count is one, and now we take ownership of that reference as a
        // drm::device::Device.
        let drm = unsafe { drm::device::Device::from_raw(raw_drm) };

        Ok(Self {
            drm,
            registered: false,
            vtable,
            fops: drm::gem::create_fops(),
            _pin: PhantomPinned,
            _p: PhantomData,
        })
    }

    /// Registers a DRM device with the rest of the kernel.
    ///
    /// Users are encouraged to use the [`drm_device_register!()`] macro because it automatically
    /// picks up the current module.
    pub fn register(
        self: Pin<&mut Self>,
        data: T::Data,
        flags: usize,
        module: &'static ThisModule,
    ) -> Result {
        if self.registered {
            // Already registered.
            return Err(EINVAL);
        }

        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        let data_pointer = <T::Data as ForeignOwnable>::into_foreign(data);
        // SAFETY: `drm` is valid per the type invariant
        unsafe {
            (*this.drm.raw_mut()).dev_private = data_pointer as *mut _;
        }

        this.fops.owner = module.0;
        this.vtable.fops = &this.fops;

        // SAFETY: The device is now initialized and ready to be registered.
        let ret = unsafe { bindings::drm_dev_register(this.drm.raw_mut(), flags as u64) };
        if ret < 0 {
            // SAFETY: `data_pointer` was returned by `into_foreign` above.
            unsafe { T::Data::from_foreign(data_pointer) };
            return Err(Error::from_kernel_errno(ret));
        }

        this.registered = true;
        Ok(())
    }

    /// Returns a reference to the `Device` instance for this registration.
    pub fn device(&self) -> &drm::device::Device<T> {
        &self.drm
    }
}

// SAFETY: `Registration` doesn't offer any methods or access to fields when shared between threads
// or CPUs, so it is safe to share it.
unsafe impl<T: Driver> Sync for Registration<T> {}

// SAFETY: Registration with and unregistration from the drm subsystem can happen from any thread.
// Additionally, `T::Data` (which is dropped during unregistration) is `Send`, so it is ok to move
// `Registration` to different threads.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: Driver> Send for Registration<T> {}

impl<T: Driver> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        if self.registered {
            // Get a pointer to the data stored in device before destroying it.
            // SAFETY: `drm` is valid per the type invariant
            let data_pointer = unsafe { (*self.drm.raw_mut()).dev_private };

            // SAFETY: Since `registered` is true, `self.drm` is both valid and registered.
            unsafe { bindings::drm_dev_unregister(self.drm.raw_mut()) };

            // Free data as well.
            // SAFETY: `data_pointer` was returned by `into_foreign` during registration.
            unsafe { <T::Data as ForeignOwnable>::from_foreign(data_pointer) };
        }
    }
}
