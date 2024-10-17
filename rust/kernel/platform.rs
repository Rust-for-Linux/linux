// SPDX-License-Identifier: GPL-2.0

//! Abstractions for the platform bus.
//!
//! C header: [`include/linux/platform_device.h`](srctree/include/linux/platform_device.h)

use crate::{
    bindings, container_of, device,
    device_id::RawDeviceId,
    driver,
    error::{to_result, Result},
    of,
    prelude::*,
    str::CStr,
    types::{ARef, ForeignOwnable},
    ThisModule,
};

/// An adapter for the registration of platform drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver + 'static> driver::RegistrationOps for Adapter<T> {
    type RegType = bindings::platform_driver;

    fn register(
        pdrv: &mut Self::RegType,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        pdrv.driver.name = name.as_char_ptr();
        pdrv.probe = Some(Self::probe_callback);

        // Both members of this union are identical in data layout and semantics.
        pdrv.__bindgen_anon_1.remove = Some(Self::remove_callback);
        pdrv.driver.of_match_table = T::ID_TABLE.as_ptr();

        // SAFETY: `pdrv` is guaranteed to be a valid `RegType`.
        to_result(unsafe { bindings::__platform_driver_register(pdrv, module.0) })
    }

    fn unregister(pdrv: &mut Self::RegType) {
        // SAFETY: `pdrv` is guaranteed to be a valid `RegType`.
        unsafe { bindings::platform_driver_unregister(pdrv) };
    }
}

impl<T: Driver + 'static> Adapter<T> {
    fn id_info(pdev: &Device) -> Option<&'static T::IdInfo> {
        let table = T::ID_TABLE;
        let id = T::of_match_device(pdev)?;

        Some(table.info(id.index()))
    }

    extern "C" fn probe_callback(pdev: *mut bindings::platform_device) -> core::ffi::c_int {
        // SAFETY: The platform bus only ever calls the probe callback with a valid `pdev`.
        let dev = unsafe { device::Device::from_raw(&mut (*pdev).dev) };
        // SAFETY: `dev` is guaranteed to be embedded in a valid `struct platform_device` by the
        // call above.
        let mut pdev = unsafe { Device::from_dev(dev) };

        let info = Self::id_info(&pdev);
        match T::probe(&mut pdev, info) {
            Ok(data) => {
                // Let the `struct platform_device` own a reference of the driver's private data.
                // SAFETY: By the type invariant `pdev.as_raw` returns a valid pointer to a
                // `struct platform_device`.
                unsafe { bindings::platform_set_drvdata(pdev.as_raw(), data.into_foreign() as _) };
            }
            Err(err) => return Error::to_errno(err),
        }

        0
    }

    extern "C" fn remove_callback(pdev: *mut bindings::platform_device) {
        // SAFETY: `pdev` is a valid pointer to a `struct platform_device`.
        let ptr = unsafe { bindings::platform_get_drvdata(pdev) };

        // SAFETY: `remove_callback` is only ever called after a successful call to
        // `probe_callback`, hence it's guaranteed that `ptr` points to a valid and initialized
        // `KBox<T>` pointer created through `KBox::into_foreign`.
        let _ = unsafe { KBox::<T>::from_foreign(ptr) };
    }
}

/// Declares a kernel module that exposes a single platform driver.
///
/// # Examples
///
/// ```ignore
/// kernel::module_platform_driver! {
///     type: MyDriver,
///     name: "Module name",
///     author: "Author name",
///     description: "Description",
///     license: "GPL v2",
/// }
/// ```
#[macro_export]
macro_rules! module_platform_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::platform::Adapter<T>, { $($f)* });
    };
}

/// IdTable type for platform drivers.
pub type IdTable<T> = &'static dyn kernel::device_id::IdTable<of::DeviceId, T>;

/// The platform driver trait.
///
/// # Example
///
///```
/// # use kernel::{bindings, c_str, of, platform};
///
/// struct MyDriver;
///
/// kernel::of_device_table!(
///     OF_TABLE,
///     MODULE_OF_TABLE,
///     <MyDriver as platform::Driver>::IdInfo,
///     [
///         (of::DeviceId::new(c_str!("redhat,my-device")), ())
///     ]
/// );
///
/// impl platform::Driver for MyDriver {
///     type IdInfo = ();
///     const ID_TABLE: platform::IdTable<Self::IdInfo> = &OF_TABLE;
///
///     fn probe(
///         _pdev: &mut platform::Device,
///         _id_info: Option<&Self::IdInfo>,
///     ) -> Result<Pin<KBox<Self>>> {
///         Err(ENODEV)
///     }
/// }
///```
/// Drivers must implement this trait in order to get a platform driver registered. Please refer to
/// the `Adapter` documentation for an example.
pub trait Driver {
    /// The type holding information about each device id supported by the driver.
    ///
    /// TODO: Use associated_type_defaults once stabilized:
    ///
    /// type IdInfo: 'static = ();
    type IdInfo: 'static;

    /// The table of device ids supported by the driver.
    const ID_TABLE: IdTable<Self::IdInfo>;

    /// Platform driver probe.
    ///
    /// Called when a new platform device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn probe(dev: &mut Device, id_info: Option<&Self::IdInfo>) -> Result<Pin<KBox<Self>>>;

    /// Find the [`of::DeviceId`] within [`Driver::ID_TABLE`] matching the given [`Device`], if any.
    fn of_match_device(pdev: &Device) -> Option<&of::DeviceId> {
        let table = Self::ID_TABLE;

        // SAFETY:
        // - `table` has static lifetime, hence it's valid for read,
        // - `dev` is guaranteed to be valid while it's alive, and so is
        //   `pdev.as_dev().as_raw()`.
        let raw_id = unsafe { bindings::of_match_device(table.as_ptr(), pdev.as_dev().as_raw()) };

        if raw_id.is_null() {
            None
        } else {
            // SAFETY: `DeviceId` is a `#[repr(transparent)` wrapper of `struct of_device_id` and
            // does not add additional invariants, so it's safe to transmute.
            Some(unsafe { &*raw_id.cast::<of::DeviceId>() })
        }
    }
}

/// The platform device representation.
///
/// A platform device is based on an always reference counted `device:Device` instance. Cloning a
/// platform device, hence, also increments the base device' reference count.
///
/// # Invariants
///
/// `Device` holds a valid reference of `ARef<device::Device>` whose underlying `struct device` is a
/// member of a `struct platform_device`.
#[derive(Clone)]
pub struct Device(ARef<device::Device>);

impl Device {
    /// Convert a raw kernel device into a `Device`
    ///
    /// # Safety
    ///
    /// `dev` must be an `Aref<device::Device>` whose underlying `bindings::device` is a member of a
    /// `bindings::platform_device`.
    unsafe fn from_dev(dev: ARef<device::Device>) -> Self {
        Self(dev)
    }

    fn as_dev(&self) -> &device::Device {
        &self.0
    }

    fn as_raw(&self) -> *mut bindings::platform_device {
        // SAFETY: By the type invariant `self.0.as_raw` is a pointer to the `struct device`
        // embedded in `struct platform_device`.
        unsafe { container_of!(self.0.as_raw(), bindings::platform_device, dev) }.cast_mut()
    }
}

impl AsRef<device::Device> for Device {
    fn as_ref(&self) -> &device::Device {
        &self.0
    }
}
