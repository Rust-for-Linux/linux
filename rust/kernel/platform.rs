// SPDX-License-Identifier: GPL-2.0

//! Platform devices and drivers.
//!
//! Also called `platdev`, `pdev`.
//!
//! C header: [`include/linux/platform_device.h`](../../../../include/linux/platform_device.h)

use crate::{
    bindings,
    device::{self, RawDevice},
    driver,
    error::{from_kernel_err_ptr, from_kernel_result, Result},
    of,
    str::CStr,
    to_result,
    types::ForeignOwnable,
    ThisModule,
};

/// A registration of a platform driver.
pub type Registration<T> = driver::Registration<Adapter<T>>;

/// An adapter for the registration of platform drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver> driver::DriverOps for Adapter<T> {
    type RegType = bindings::platform_driver;

    unsafe fn register(
        reg: *mut bindings::platform_driver,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` is non-null and valid.
        let pdrv = unsafe { &mut *reg };

        pdrv.driver.name = name.as_char_ptr();
        pdrv.probe = Some(Self::probe_callback);
        pdrv.remove = Some(Self::remove_callback);
        if let Some(t) = T::OF_DEVICE_ID_TABLE {
            pdrv.driver.of_match_table = t.as_ref();
        }
        // SAFETY:
        //   - `pdrv` lives at least until the call to `platform_driver_unregister()` returns.
        //   - `name` pointer has static lifetime.
        //   - `module.0` lives at least as long as the module.
        //   - `probe()` and `remove()` are static functions.
        //   - `of_match_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`driver::IdTable`] type, or null.
        to_result(unsafe { bindings::__platform_driver_register(reg, module.0) })
    }

    unsafe fn unregister(reg: *mut bindings::platform_driver) {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` was passed (and updated) by a previous successful call to
        // `platform_driver_register`.
        unsafe { bindings::platform_driver_unregister(reg) };
    }
}

impl<T: Driver> Adapter<T> {
    fn get_id_info(dev: &Device) -> Option<&'static T::IdInfo> {
        let table = T::OF_DEVICE_ID_TABLE?;

        // SAFETY: `table` has static lifetime, so it is valid for read. `dev` is guaranteed to be
        // valid while it's alive, so is the raw device returned by it.
        let id = unsafe { bindings::of_match_device(table.as_ref(), dev.raw_device()) };
        if id.is_null() {
            return None;
        }

        // SAFETY: `id` is a pointer within the static table, so it's always valid.
        let offset = unsafe { (*id).data };
        if offset.is_null() {
            return None;
        }

        // SAFETY: The offset comes from a previous call to `offset_from` in `IdArray::new`, which
        // guarantees that the resulting pointer is within the table.
        let ptr = unsafe {
            id.cast::<u8>()
                .offset(offset as _)
                .cast::<Option<T::IdInfo>>()
        };

        // SAFETY: The id table has a static lifetime, so `ptr` is guaranteed to be valid for read.
        #[allow(clippy::needless_borrow)]
        unsafe {
            (&*ptr).as_ref()
        }
    }

    extern "C" fn probe_callback(pdev: *mut bindings::platform_device) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `pdev` is valid by the contract with the C code. `dev` is alive only for the
            // duration of this call, so it is guaranteed to remain alive for the lifetime of
            // `pdev`.
            let mut dev = unsafe { Device::from_ptr(pdev) };
            let info = Self::get_id_info(&dev);
            let data = T::probe(&mut dev, info)?;
            // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
            unsafe { bindings::platform_set_drvdata(pdev, data.into_foreign() as _) };
            Ok(0)
        }
    }

    extern "C" fn remove_callback(pdev: *mut bindings::platform_device) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
            let ptr = unsafe { bindings::platform_get_drvdata(pdev) };
            // SAFETY:
            //   - we allocated this pointer using `T::Data::into_foreign`,
            //     so it is safe to turn back into a `T::Data`.
            //   - the allocation happened in `probe`, no-one freed the memory,
            //     `remove` is the canonical kernel location to free driver data. so OK
            //     to convert the pointer back to a Rust structure here.
            let data = unsafe { T::Data::from_foreign(ptr) };
            let ret = T::remove(&data);
            <T::Data as driver::DeviceRemoval>::device_remove(&data);
            ret?;
            Ok(0)
        }
    }
}

/// A platform driver.
pub trait Driver {
    /// Data stored on device by driver.
    ///
    /// Corresponds to the data set or retrieved via the kernel's
    /// `platform_{set,get}_drvdata()` functions.
    ///
    /// Require that `Data` implements `ForeignOwnable`. We guarantee to
    /// never move the underlying wrapped data structure. This allows
    type Data: ForeignOwnable + Send + Sync + driver::DeviceRemoval = ();

    /// The type holding information about each device id supported by the driver.
    type IdInfo: 'static = ();

    /// The table of device ids supported by the driver.
    const OF_DEVICE_ID_TABLE: Option<driver::IdTable<'static, of::DeviceId, Self::IdInfo>> = None;

    /// Platform driver probe.
    ///
    /// Called when a new platform device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn probe(dev: &mut Device, id_info: Option<&Self::IdInfo>) -> Result<Self::Data>;

    /// Platform driver remove.
    ///
    /// Called when a platform device is removed.
    /// Implementers should prepare the device for complete removal here.
    fn remove(_data: &Self::Data) -> Result {
        Ok(())
    }
}

/// A platform device.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Device {
    ptr: *mut bindings::platform_device,
    is_registered: bool,
}

// SAFETY: `Device` only holds a pointer to a C device, which is safe to be used from any thread.
unsafe impl Send for Device {}

// SAFETY: `Device` only holds a pointer to a C device, references to which are safe to be used
// from any thread.
unsafe impl Sync for Device {}

impl Device {
    /// Creates a new device from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. It must remain valid for the lifetime of the returned
    /// instance.
    unsafe fn from_ptr(ptr: *mut bindings::platform_device) -> Self {
        // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
        Self {
            ptr,
            is_registered: false,
        }
    }

    /// Add a platform-level device and its resources
    pub fn register(name: &'static CStr, id: i32) -> Result<Self> {
        // SAFETY: It returns a non-null and valid pointer to a struct platform_device
        let pdev = from_kernel_err_ptr(unsafe {
            bindings::platform_device_register_simple(name.as_char_ptr(), id, core::ptr::null(), 0)
        })?;

        Ok(Self {
            ptr: pdev,
            is_registered: true,
        })
    }

    /// Returns id of the platform device.
    pub fn id(&self) -> i32 {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { (*self.ptr).id }
    }

    /// Similar to the above, except it deals with the case where the device
    /// does not have dev->dma_mask appropriately setup.
    pub fn coerse_dma_masks(&mut self, mask: u64) -> Result {
        to_result(unsafe { bindings::dma_coerce_mask_and_coherent(&mut (*self.ptr).dev, mask) })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if self.is_registered {
            // SAFETY: This path only runs if a previous call to `register` completed
            // successfully.
            unsafe { bindings::platform_device_unregister(self.ptr) };
        }
    }
}
// SAFETY: The device returned by `raw_device` is the raw platform device.
unsafe impl device::RawDevice for Device {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.ptr).dev }
    }
}

/// Declares a kernel module that exposes a single platform driver.
///
/// # Examples
///
/// ```ignore
/// # use kernel::{platform, define_of_id_table, module_platform_driver};
/// #
/// struct MyDriver;
/// impl platform::Driver for MyDriver {
///     // [...]
/// #   fn probe(_dev: &mut platform::Device, _id_info: Option<&Self::IdInfo>) -> Result {
/// #       Ok(())
/// #   }
/// #   define_of_id_table! {(), [
/// #       (of::DeviceId::Compatible(b"brcm,bcm2835-rng"), None),
/// #   ]}
/// }
///
/// module_platform_driver! {
///     type: MyDriver,
///     name: "module_name",
///     author: "Author name",
///     license: "GPL",
/// }
/// ```
#[macro_export]
macro_rules! module_platform_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::platform::Adapter<T>, { $($f)* });
    };
}
