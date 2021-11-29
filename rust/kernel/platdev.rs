// SPDX-License-Identifier: GPL-2.0

//! Platform devices.
//!
//! Also called `platdev`, `pdev`.
//!
//! C header: [`include/linux/platform_device.h`](../../../../include/linux/platform_device.h)

use crate::{
    bindings, c_types, device,
    error::{from_kernel_err_ptr, from_kernel_result, Error, Result},
    of::OfMatchTable,
    str::CStr,
    types::PointerWrapper,
};

use alloc::boxed::Box;
use core::{marker::PhantomPinned, pin::Pin};

/// Platform device, corresponds to `struct platform_device`.
///
/// # Invariants
///
/// The pointer is valid.
pub struct PlatformDevice(*mut bindings::platform_device);

// SAFETY: The device returned by `raw_device` is the PlatformDevice.
unsafe impl device::RawDevice for PlatformDevice {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.0).dev }
    }
}

/// Ioremaps resources of a platform device.
pub fn devm_platform_ioremap_resource(
    pdev: &mut PlatformDevice,
    index: u32,
) -> Result<*mut c_types::c_void> {
    // SAFETY: `pdev` is valid by the type invariant.
    unsafe { from_kernel_err_ptr(bindings::devm_platform_ioremap_resource(pdev.0, index)) }
}

/// A registration of a platform device.
#[derive(Default)]
pub struct Registration {
    registered: bool,
    pdrv: bindings::platform_driver,
    _pin: PhantomPinned,
}

// SAFETY: `Registration` does not expose any of its state across threads
// (it is fine for multiple threads to have a shared reference to it).
unsafe impl Sync for Registration {}

extern "C" fn probe_callback<P: PlatformDriver>(
    pdev: *mut bindings::platform_device,
) -> c_types::c_int {
    from_kernel_result! {
        // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
        let mut platdev = PlatformDevice(pdev);
        let drv_data = P::probe(&mut platdev)?;
        let drv_data = drv_data.into_pointer() as *mut c_types::c_void;
        // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
        unsafe {
            bindings::platform_set_drvdata(pdev, drv_data);
        }
        Ok(0)
    }
}

extern "C" fn remove_callback<P: PlatformDriver>(
    pdev: *mut bindings::platform_device,
) -> c_types::c_int {
    from_kernel_result! {
        // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
        let mut platdev = PlatformDevice(pdev);
        // SAFETY: `pdev` is guaranteed to be a valid, non-null pointer.
        let ptr = unsafe { bindings::platform_get_drvdata(pdev) };
        // SAFETY:
        //   - we allocated this pointer using `P::DrvData::into_pointer`,
        //     so it is safe to turn back into a `P::DrvData`.
        //   - the allocation happened in `probe`, no-one freed the memory,
        //     `remove` is the canonical kernel location to free driver data. so OK
        //     to convert the pointer back to a Rust structure here.
        let drv_data = unsafe { P::DrvData::from_pointer(ptr) };
        P::remove(&mut platdev, drv_data)?;
        Ok(0)
    }
}

impl Registration {
    fn register<P: PlatformDriver>(
        self: Pin<&mut Self>,
        name: &'static CStr,
        of_match_table: Option<&'static OfMatchTable>,
        module: &'static crate::ThisModule,
    ) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            // Already registered.
            return Err(Error::EINVAL);
        }
        this.pdrv.driver.name = name.as_char_ptr();
        if let Some(tbl) = of_match_table {
            this.pdrv.driver.of_match_table = tbl.as_ptr();
        }
        this.pdrv.probe = Some(probe_callback::<P>);
        this.pdrv.remove = Some(remove_callback::<P>);
        // SAFETY:
        //   - `this.pdrv` lives at least until the call to `platform_driver_unregister()` returns.
        //   - `name` pointer has static lifetime.
        //   - `module.0` lives at least as long as the module.
        //   - `probe()` and `remove()` are static functions.
        //   - `of_match_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`of::OfMatchTable::as_ptr()`] return type,
        //      or null.
        let ret = unsafe { bindings::__platform_driver_register(&mut this.pdrv, module.0) };
        if ret < 0 {
            return Err(Error::from_kernel_errno(ret));
        }
        this.registered = true;
        Ok(())
    }

    /// Registers a platform device.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned<P: PlatformDriver>(
        name: &'static CStr,
        of_match_tbl: Option<&'static OfMatchTable>,
        module: &'static crate::ThisModule,
    ) -> Result<Pin<Box<Self>>> {
        let mut r = Pin::from(Box::try_new(Self::default())?);
        r.as_mut().register::<P>(name, of_match_tbl, module)?;
        Ok(r)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.registered {
            // SAFETY: if `registered` is true, then `self.pdev` was registered
            // previously, which means `platform_driver_unregister` is always
            // safe to call.
            unsafe { bindings::platform_driver_unregister(&mut self.pdrv) }
        }
    }
}

/// Trait for implementers of platform drivers.
///
/// Implement this trait whenever you create a platform driver.
pub trait PlatformDriver {
    /// Device driver data.
    ///
    /// Corresponds to the data set or retrieved via the kernel's
    /// `platform_{set,get}_drvdata()` functions.
    ///
    /// Require that `DrvData` implements `PointerWrapper`. We guarantee to
    /// never move the underlying wrapped data structure. This allows
    /// driver writers to use pinned or self-referential data structures.
    type DrvData: PointerWrapper;

    /// Platform driver probe.
    ///
    /// Called when a new platform device is added or discovered.
    /// Implementers should attempt to initialize the device here.
    fn probe(pdev: &mut PlatformDevice) -> Result<Self::DrvData>;

    /// Platform driver remove.
    ///
    /// Called when a platform device is removed.
    /// Implementers should prepare the device for complete removal here.
    fn remove(pdev: &mut PlatformDevice, drv_data: Self::DrvData) -> Result;
}
