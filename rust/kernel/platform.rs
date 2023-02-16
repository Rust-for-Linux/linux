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
    error::{code::*, from_kernel_result, to_result, Result},
    io_mem::{IoMem, IoResource, Resource},
    of,
    str::CStr,
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
    used_resource: u64,
}

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
            used_resource: 0,
        }
    }

    /// Returns id of the platform device.
    pub fn id(&self) -> i32 {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { (*self.ptr).id }
    }

    /// Sets the DMA masks (normal and coherent) for a platform device.
    pub fn set_dma_masks(&mut self, mask: u64) -> Result {
        to_result(unsafe { bindings::dma_set_mask_and_coherent(&mut (*self.ptr).dev, mask) })
    }

    /// Gets a system resources of a platform device.
    pub fn get_resource(&mut self, rtype: IoResource, num: usize) -> Result<Resource> {
        // SAFETY: `self.ptr` is valid by the type invariant.
        let res = unsafe { bindings::platform_get_resource(self.ptr, rtype as _, num as _) };
        if res.is_null() {
            return Err(EINVAL);
        }

        // Get the position of the found resource in the array.
        // SAFETY:
        //   - `self.ptr` is valid by the type invariant.
        //   - `res` is a displaced pointer to one of the array's elements,
        //     and `resource` is its base pointer.
        let index = unsafe { res.offset_from((*self.ptr).resource) } as usize;

        // Make sure that the index does not exceed the 64-bit mask.
        assert!(index < 64);

        if self.used_resource >> index & 1 == 1 {
            return Err(EBUSY);
        }
        self.used_resource |= 1 << index;

        // SAFETY: The pointer `res` is returned from `bindings::platform_get_resource`
        // above and checked if it is not a NULL.
        unsafe { Resource::new((*res).start, (*res).end, (*res).flags) }.ok_or(EINVAL)
    }

    /// Ioremaps resources of a platform device.
    ///
    /// # Safety
    ///
    /// Callers must ensure that either (a) the resulting interface cannot be used to initiate DMA
    /// operations, or (b) that DMA operations initiated via the returned interface use DMA handles
    /// allocated through the `dma` module.
    pub unsafe fn ioremap_resource<const SIZE: usize>(
        &mut self,
        index: usize,
    ) -> Result<IoMem<SIZE>> {
        let mask = self.used_resource;
        let res = self.get_resource(IoResource::Mem, index)?;

        // SAFETY: Valid by the safety contract.
        let iomem = unsafe { IoMem::<SIZE>::try_new(res) };
        // If remapping fails, the given resource won't be used, so restore the old mask.
        if iomem.is_err() {
            self.used_resource = mask;
        }
        iomem
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
