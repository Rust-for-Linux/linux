// SPDX-License-Identifier: GPL-2.0

//! Abstractions for the PCI bus.
//!
//! C header: [`include/linux/pci.h`](srctree/include/linux/pci.h)

use crate::{
    bindings, container_of, device,
    device_id::RawDeviceId,
    driver,
    error::{to_result, Result},
    str::CStr,
    types::{ARef, ForeignOwnable},
    ThisModule,
};
use core::ops::Deref;
use kernel::prelude::*;

/// An adapter for the registration of PCI drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver + 'static> driver::RegistrationOps for Adapter<T> {
    type RegType = bindings::pci_driver;

    fn register(
        pdrv: &mut Self::RegType,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        pdrv.name = name.as_char_ptr();
        pdrv.probe = Some(Self::probe_callback);
        pdrv.remove = Some(Self::remove_callback);
        pdrv.id_table = T::ID_TABLE.as_ptr();

        // SAFETY: `pdrv` is guaranteed to be a valid `RegType`.
        to_result(unsafe {
            bindings::__pci_register_driver(pdrv as _, module.0, name.as_char_ptr())
        })
    }

    fn unregister(pdrv: &mut Self::RegType) {
        // SAFETY: `pdrv` is guaranteed to be a valid `RegType`.
        unsafe { bindings::pci_unregister_driver(pdrv) }
    }
}

impl<T: Driver + 'static> Adapter<T> {
    extern "C" fn probe_callback(
        pdev: *mut bindings::pci_dev,
        id: *const bindings::pci_device_id,
    ) -> core::ffi::c_int {
        // SAFETY: The PCI bus only ever calls the probe callback with a valid pointer to a
        // `struct pci_dev`.
        let dev = unsafe { device::Device::from_raw(&mut (*pdev).dev) };
        // SAFETY: `dev` is guaranteed to be embedded in a valid `struct pci_dev` by the call
        // above.
        let mut pdev = unsafe { Device::from_dev(dev) };

        // SAFETY: `DeviceId` is a `#[repr(transparent)` wrapper of `struct pci_device_id` and
        // does not add additional invariants, so it's safe to transmute.
        let id = unsafe { &*id.cast::<DeviceId>() };
        let info = T::ID_TABLE.info(id.index());

        match T::probe(&mut pdev, id, info) {
            Ok(data) => {
                // Let the `struct pci_dev` own a reference of the driver's private data.
                // SAFETY: By the type invariant `pdev.as_raw` returns a valid pointer to a
                // `struct pci_dev`.
                unsafe { bindings::pci_set_drvdata(pdev.as_raw(), data.into_foreign() as _) };
            }
            Err(err) => return Error::to_errno(err),
        }

        0
    }

    extern "C" fn remove_callback(pdev: *mut bindings::pci_dev) {
        // SAFETY: The PCI bus only ever calls the remove callback with a valid pointer to a
        // `struct pci_dev`.
        let ptr = unsafe { bindings::pci_get_drvdata(pdev) };

        // SAFETY: `remove_callback` is only ever called after a successful call to
        // `probe_callback`, hence it's guaranteed that `ptr` points to a valid and initialized
        // `KBox<T>` pointer created through `KBox::into_foreign`.
        let _ = unsafe { KBox::<T>::from_foreign(ptr) };
    }
}

/// Declares a kernel module that exposes a single PCI driver.
///
/// # Example
///
///```ignore
/// kernel::module_pci_driver! {
///     type: MyDriver,
///     name: "Module name",
///     author: "Author name",
///     description: "Description",
///     license: "GPL v2",
/// }
///```
#[macro_export]
macro_rules! module_pci_driver {
($($f:tt)*) => {
    $crate::module_driver!(<T>, $crate::pci::Adapter<T>, { $($f)* });
};
}

/// Abstraction for bindings::pci_device_id.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct DeviceId(bindings::pci_device_id);

impl DeviceId {
    const PCI_ANY_ID: u32 = !0;

    /// PCI_DEVICE macro.
    pub const fn new(vendor: u32, device: u32) -> Self {
        Self(bindings::pci_device_id {
            vendor,
            device,
            subvendor: DeviceId::PCI_ANY_ID,
            subdevice: DeviceId::PCI_ANY_ID,
            class: 0,
            class_mask: 0,
            driver_data: 0,
            override_only: 0,
        })
    }

    /// PCI_DEVICE_CLASS macro.
    pub const fn with_class(class: u32, class_mask: u32) -> Self {
        Self(bindings::pci_device_id {
            vendor: DeviceId::PCI_ANY_ID,
            device: DeviceId::PCI_ANY_ID,
            subvendor: DeviceId::PCI_ANY_ID,
            subdevice: DeviceId::PCI_ANY_ID,
            class,
            class_mask,
            driver_data: 0,
            override_only: 0,
        })
    }
}

// Allow drivers R/O access to the fields of `pci_device_id`; should we prefer accessor functions
// to void exposing C structure fields?
impl Deref for DeviceId {
    type Target = bindings::pci_device_id;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// SAFETY:
// * `DeviceId` is a `#[repr(transparent)` wrapper of `pci_device_id` and does not add
//   additional invariants, so it's safe to transmute to `RawType`.
// * `DRIVER_DATA_OFFSET` is the offset to the `driver_data` field.
unsafe impl RawDeviceId for DeviceId {
    type RawType = bindings::pci_device_id;

    const DRIVER_DATA_OFFSET: usize = core::mem::offset_of!(bindings::pci_device_id, driver_data);

    fn index(&self) -> usize {
        self.driver_data as _
    }
}

/// IdTable type for PCI
pub type IdTable<T> = &'static dyn kernel::device_id::IdTable<DeviceId, T>;

/// Create a PCI `IdTable` with its alias for modpost.
#[macro_export]
macro_rules! pci_device_table {
    ($table_name:ident, $module_table_name:ident, $id_info_type: ty, $table_data: expr) => {
        const $table_name: $crate::device_id::IdArray<
            $crate::pci::DeviceId,
            $id_info_type,
            { $table_data.len() },
        > = $crate::device_id::IdArray::new($table_data);

        $crate::module_device_table!("pci", $module_table_name, $table_name);
    };
}

/// The PCI driver trait.
///
/// # Example
///
///```
/// # use kernel::{bindings, pci};
///
/// struct MyDriver;
///
/// kernel::pci_device_table!(
///     PCI_TABLE,
///     MODULE_PCI_TABLE,
///     <MyDriver as pci::Driver>::IdInfo,
///     [
///         (pci::DeviceId::new(bindings::PCI_VENDOR_ID_REDHAT, bindings::PCI_ANY_ID as u32), ())
///     ]
/// );
///
/// impl pci::Driver for MyDriver {
///     type IdInfo = ();
///     const ID_TABLE: pci::IdTable<Self::IdInfo> = &PCI_TABLE;
///
///     fn probe(
///         _pdev: &mut pci::Device,
///         _id: &pci::DeviceId,
///         _id_info: &Self::IdInfo,
///     ) -> Result<Pin<KBox<Self>>> {
///         Err(ENODEV)
///     }
/// }
///```
/// Drivers must implement this trait in order to get a PCI driver registered. Please refer to the
/// `Adapter` documentation for an example.
pub trait Driver {
    /// The type holding information about each device id supported by the driver.
    ///
    /// TODO: Use associated_type_defaults once stabilized:
    ///
    /// type IdInfo: 'static = ();
    type IdInfo: 'static;

    /// The table of device ids supported by the driver.
    const ID_TABLE: IdTable<Self::IdInfo>;

    /// PCI driver probe.
    ///
    /// Called when a new platform device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn probe(dev: &mut Device, id: &DeviceId, id_info: &Self::IdInfo) -> Result<Pin<KBox<Self>>>;
}

/// The PCI device representation.
///
/// A PCI device is based on an always reference counted `device:Device` instance. Cloning a PCI
/// device, hence, also increments the base device' reference count.
#[derive(Clone)]
pub struct Device(ARef<device::Device>);

impl Device {
    /// Create a PCI Device instance from an existing `device::Device`.
    ///
    /// # Safety
    ///
    /// `dev` must be an `ARef<device::Device>` whose underlying `bindings::device` is a member of
    /// a `bindings::pci_dev`.
    pub unsafe fn from_dev(dev: ARef<device::Device>) -> Self {
        Self(dev)
    }

    fn as_raw(&self) -> *mut bindings::pci_dev {
        // SAFETY: By the type invariant `self.0.as_raw` is a pointer to the `struct device`
        // embedded in `struct pci_dev`.
        unsafe { container_of!(self.0.as_raw(), bindings::pci_dev, dev) as _ }
    }

    /// Enable memory resources for this device.
    pub fn enable_device_mem(&self) -> Result {
        // SAFETY: `self.as_raw` is guaranteed to be a pointer to a valid `struct pci_dev`.
        let ret = unsafe { bindings::pci_enable_device_mem(self.as_raw()) };
        if ret != 0 {
            Err(Error::from_errno(ret))
        } else {
            Ok(())
        }
    }

    /// Enable bus-mastering for this device.
    pub fn set_master(&self) {
        // SAFETY: `self.as_raw` is guaranteed to be a pointer to a valid `struct pci_dev`.
        unsafe { bindings::pci_set_master(self.as_raw()) };
    }
}

impl AsRef<device::Device> for Device {
    fn as_ref(&self) -> &device::Device {
        &self.0
    }
}
