// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM driver core.
//!
//! C header: [`include/linux/drm/drm_drv.h`](srctree/include/linux/drm/drm_drv.h)

use crate::{bindings, drm, private::Sealed, str::CStr, types::ForeignOwnable};
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
/// Driver supports compute acceleration devices. This flag is mutually exclusive with `FEAT_RENDER`
/// and `FEAT_MODESET`. Devices that support both graphics and compute acceleration should be
/// handled by two drivers that are connected using auxiliary bus.
pub const FEAT_COMPUTE_ACCEL: u32 = bindings::drm_driver_feature_DRIVER_COMPUTE_ACCEL;
/// Driver supports user defined GPU VA bindings for GEM objects.
pub const FEAT_GEM_GPUVA: u32 = bindings::drm_driver_feature_DRIVER_GEM_GPUVA;
/// Driver supports and requires cursor hotspot information in the cursor plane (e.g. cursor plane
/// has to actually track the mouse cursor and the clients are required to set hotspot in order for
/// the cursor planes to work correctly).
pub const FEAT_CURSOR_HOTSPOT: u32 = bindings::drm_driver_feature_DRIVER_CURSOR_HOTSPOT;

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
}

/// Trait for memory manager implementations. Implemented internally.
pub trait AllocImpl: Sealed {
    /// The C callback operations for this memory manager.
    const ALLOC_OPS: AllocOps;
}

/// The DRM `Driver` trait.
///
/// This trait must be implemented by drivers in order to create a `struct drm_device` and `struct
/// drm_driver` to be registered in the DRM subsystem.
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

    /// Driver metadata
    const INFO: DriverInfo;

    /// Feature flags
    const FEATURES: u32;

    /// IOCTL list. See `kernel::drm::ioctl::declare_drm_ioctls!{}`.
    const IOCTLS: &'static [drm::ioctl::DrmIoctlDescriptor];
}
