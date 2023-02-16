// SPDX-License-Identifier: GPL-2.0 OR MIT
#![allow(non_snake_case)]

//! DRM IOCTL definitions.
//!
//! C header: [`include/linux/drm/drm_ioctl.h`](../../../../include/linux/drm/drm_ioctl.h)

use crate::ioctl;

const BASE: u32 = bindings::DRM_IOCTL_BASE as u32;

/// Construct a DRM ioctl number with no argument.
pub const fn IO(nr: u32) -> u32 {
    ioctl::_IO(BASE, nr)
}

/// Construct a DRM ioctl number with a read-only argument.
pub const fn IOR<T>(nr: u32) -> u32 {
    ioctl::_IOR::<T>(BASE, nr)
}

/// Construct a DRM ioctl number with a write-only argument.
pub const fn IOW<T>(nr: u32) -> u32 {
    ioctl::_IOW::<T>(BASE, nr)
}

/// Construct a DRM ioctl number with a read-write argument.
pub const fn IOWR<T>(nr: u32) -> u32 {
    ioctl::_IOWR::<T>(BASE, nr)
}

/// Descriptor type for DRM ioctls. Use the `declare_drm_ioctls!{}` macro to construct them.
pub type DrmIoctlDescriptor = bindings::drm_ioctl_desc;

/// This is for ioctl which are used for rendering, and require that the file descriptor is either
/// for a render node, or if it’s a legacy/primary node, then it must be authenticated.
pub const AUTH: u32 = bindings::drm_ioctl_flags_DRM_AUTH;

/// This must be set for any ioctl which can change the modeset or display state. Userspace must
/// call the ioctl through a primary node, while it is the active master.
///
/// Note that read-only modeset ioctl can also be called by unauthenticated clients, or when a
/// master is not the currently active one.
pub const MASTER: u32 = bindings::drm_ioctl_flags_DRM_MASTER;

/// Anything that could potentially wreak a master file descriptor needs to have this flag set.
///
/// Current that’s only for the SETMASTER and DROPMASTER ioctl, which e.g. logind can call to force
/// a non-behaving master (display compositor) into compliance.
///
/// This is equivalent to callers with the SYSADMIN capability.
pub const ROOT_ONLY: u32 = bindings::drm_ioctl_flags_DRM_ROOT_ONLY;

/// Whether drm_ioctl_desc.func should be called with the DRM BKL held or not. Enforced as the
/// default for all modern drivers, hence there should never be a need to set this flag.
///
/// Do not use anywhere else than for the VBLANK_WAIT IOCTL, which is the only legacy IOCTL which
/// needs this.
pub const UNLOCKED: u32 = bindings::drm_ioctl_flags_DRM_UNLOCKED;

/// This is used for all ioctl needed for rendering only, for drivers which support render nodes.
/// This should be all new render drivers, and hence it should be always set for any ioctl with
/// `AUTH` set. Note though that read-only query ioctl might have this set, but have not set
/// DRM_AUTH because they do not require authentication.
pub const RENDER_ALLOW: u32 = bindings::drm_ioctl_flags_DRM_RENDER_ALLOW;

/// Declare the DRM ioctls for a driver.
///
/// Each entry in the list should have the form:
///
/// `(ioctl_number, argument_type, flags, user_callback),`
///
/// `argument_type` is the type name within the `bindings` crate.
/// `user_callback` should have the following prototype:
///
/// ```
/// fn foo(device: &kernel::drm::device::Device<Self>,
///        data: &mut bindings::argument_type,
///        file: &kernel::drm::file::File<Self::File>,
/// )
/// ```
/// where `Self` is the drm::drv::Driver implementation these ioctls are being declared within.
///
/// # Examples
///
/// ```
/// kernel::declare_drm_ioctls! {
///     (FOO_GET_PARAM, drm_foo_get_param, ioctl::RENDER_ALLOW, my_get_param_handler),
/// }
/// ```
///
#[macro_export]
macro_rules! declare_drm_ioctls {
    ( $(($cmd:ident, $struct:ident, $flags:expr, $func:expr)),* $(,)? ) => {
        const IOCTLS: &'static [$crate::drm::ioctl::DrmIoctlDescriptor] = {
            const _:() = {
                use $crate::bindings::*;
                let i: u32 = $crate::bindings::DRM_COMMAND_BASE;
                // Assert that all the IOCTLs are in the right order and there are no gaps,
                // and that the sizeof of the specified type is correct.
                $(
                    let cmd: u32 = $crate::macros::concat_idents!(DRM_IOCTL_, $cmd);
                    ::core::assert!(i == $crate::ioctl::_IOC_NR(cmd));
                    ::core::assert!(core::mem::size_of::<$crate::bindings::$struct>() == $crate::ioctl::_IOC_SIZE(cmd));
                    let i: u32 = i + 1;
                )*
            };

            let ioctls = &[$(
                $crate::bindings::drm_ioctl_desc {
                    // TODO: nicer solution to this?
                    cmd: {
                        use $crate::bindings::*;
                        $crate::macros::concat_idents!(DRM_IOCTL_, $cmd) as u32
                    },
                    func: {
                        #[allow(non_snake_case)]
                        unsafe extern "C" fn $cmd(
                                raw_dev: *mut $crate::bindings::drm_device,
                                raw_data: *mut ::core::ffi::c_void,
                                raw_file_priv: *mut $crate::bindings::drm_file,
                        ) -> core::ffi::c_int {
                            // SAFETY: We never drop this, and the DRM core ensures the device lives
                            // while callbacks are being called.
                            //
                            // FIXME: Currently there is nothing enforcing that the types of the
                            // dev/file match the current driver these ioctls are being declared
                            // for, and it's not clear how to enforce this within the type system.
                            let dev = ::core::mem::ManuallyDrop::new(unsafe {
                                $crate::drm::device::Device::from_raw(raw_dev)
                            });
                            // SAFETY: This is just the ioctl argument, which hopefully has the right type
                            // (we've done our best checking the size).
                            let data = unsafe { &mut *(raw_data as *mut $crate::bindings::$struct) };
                            // SAFETY: This is just the DRM file structure
                            let file = unsafe { $crate::drm::file::File::from_raw(raw_file_priv) };

                            match $func(&*dev, data, &file) {
                                Err(e) => e.to_kernel_errno(),
                                Ok(i) => i.try_into().unwrap_or(ERANGE.to_kernel_errno()),
                            }
                        }
                        Some($cmd)
                    },
                    flags: $flags,
                    name: $crate::c_str!(::core::stringify!($cmd)).as_char_ptr(),
                }
            ),*];
            ioctls
        };
    };
}
