// SPDX-License-Identifier: GPL-2.0

//! Virtio.
//!
//! C header: [`include/linux/virtio.h`](../../../../include/linux/virtio.h)

use crate::{
    bindings, device, driver, error::from_kernel_result, str::CStr, to_result,
    types::PointerWrapper, Result, ThisModule,
};

/// A registration of a virtio driver.
pub type Registration<T> = driver::Registration<Adapter<T>>;

/// Id of a Virtio device.
#[derive(Clone, Copy)]
pub struct DeviceId {
    /// Device id.
    pub device: u32,

    /// Vendor id of the virtio device.
    pub vendor: u32,
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores in `virtio_id::data`.
unsafe impl const driver::RawDeviceId for DeviceId {
    type RawType = bindings::virtio_device_id;
    const ZERO: Self::RawType = bindings::virtio_device_id {
        device: 0,
        vendor: 0,
    };

    fn to_rawid(&self, _offset: isize) -> Self::RawType {
        bindings::virtio_device_id {
            device: self.device,
            vendor: self.vendor,
        }
    }
}

/// An adapter for the registration of virtio drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver> driver::DriverOps for Adapter<T> {
    type RegType = bindings::virtio_driver;

    unsafe fn register(
        reg: *mut bindings::virtio_driver,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` is non-null and valid.
        let vdrv = unsafe { &mut *reg };

        vdrv.driver.name = name.as_char_ptr();
        vdrv.driver.owner = module.0;
        vdrv.probe = Some(Self::probe_callback);
        vdrv.remove = Some(Self::remove_callback);
        if let Some(t) = T::ID_TABLE {
            vdrv.id_table = t.as_ref();
        }
        // SAFETY:
        //   - `vdrv` lives at least until the call to `register_virtio_driver()` returns.
        //   - `name` pointer has static lifetime.
        //   - `module.0` lives at least as long as the module.
        //   - `probe()` and `remove()` are static functions.
        //   - `id_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`driver::IdTable`] type, or null.
        to_result(unsafe { bindings::register_virtio_driver(reg) })
    }

    unsafe fn unregister(reg: *mut bindings::virtio_driver) {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` was passed (and updated) by a previous successful call to
        // `register_virtio_driver`.
        unsafe { bindings::unregister_virtio_driver(reg) };
    }
}

impl<T: Driver> Adapter<T> {
    extern "C" fn probe_callback(vdev: *mut bindings::virtio_device) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `vdev` is valid by the contract with the C code. `dev` is alive only for the
            // duration of this call, so it is guaranteed to remain alive for the lifetime of
            // `vdev`.
            let mut dev = unsafe { Device::from_ptr(vdev) };
            let data = T::probe(&mut dev)?;
            // SAFETY: `vdev` is guaranteed to be a valid, non-null pointer.
            unsafe{(*vdev).priv_ = T::Data::into_pointer(data) as _;}
            Ok(0)
        }
    }

    extern "C" fn remove_callback(vdev: *mut bindings::virtio_device) {
        // SAFETY: `vdev` is guaranteed to be a valid, non-null pointer.
        let ptr = unsafe { *vdev }.priv_;
        // SAFETY:
        //   - we allocated this pointer using `T::Data::into_pointer`,
        //     so it is safe to turn back into a `T::Data`.
        //   - the allocation happened in `probe`, no-one freed the memory,
        //     `remove` is the canonical kernel location to free driver data. so OK
        //     to convert the pointer back to a Rust structure here.
        let data = unsafe { T::Data::from_pointer(ptr) };
        T::remove(&data);
        <T::Data as driver::DeviceRemoval>::device_remove(&data);
    }
}

/// A virtio driver.
pub trait Driver {
    /// Data stored on device by driver.
    type Data: PointerWrapper + Send + Sync + driver::DeviceRemoval = ();

    /// The table of device ids supported by the driver.
    const ID_TABLE: Option<driver::IdTable<'static, DeviceId, ()>> = None;

    /// Probes for the device with the given id.
    fn probe(dev: &mut Device) -> Result<Self::Data>;

    /// Cleans any resources up that are associated with the device.
    ///
    /// This is called when the driver is detached from the device.
    fn remove(_data: &Self::Data) {}
}

/// A Virtio device.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Device {
    ptr: *mut bindings::virtio_device,
}

impl Device {
    /// Creates a new device from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. It must remain valid for the lifetime of the returned
    /// instance.
    unsafe fn from_ptr(ptr: *mut bindings::virtio_device) -> Self {
        // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
        Self { ptr }
    }
}

// SAFETY: The device returned by `raw_device` is the raw virtio device.
unsafe impl device::RawDevice for Device {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.ptr).dev }
    }
}

/// Declares a kernel module that exposes a single virtio driver.
///
/// # Examples
///
/// ```
/// # use kernel::{virtio, define_virtio_id_table, module_virtio_driver};
/// #
/// struct MyDriver;
/// impl virtio::Driver for MyDriver {
///     // [...]
/// #   fn probe(_dev: &mut virtio::Device) -> Result {
/// #       Ok(())
/// #   }
/// #   define_virtio_id_table! {(), [
/// #       ({ device: 0x00000001, vendor: 0xffffffff }, None),
/// #   ]}
/// }
///
/// module_virtio_driver! {
///     type: MyDriver,
///     name: b"module_name",
///     author: b"Author name",
///     license: b"GPL",
/// }
/// ```
#[macro_export]
macro_rules! module_virtio_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::virtio::Adapter<T>, { $($f)* });
    };
}

/// Defines the id table for virtio devices.
///
/// # Examples
///
/// ```
/// # use kernel::{virtio, define_virtio_id_table};
/// #
/// # struct Sample;
/// # impl kernel::virtio::Driver for Sample {
/// #   fn probe(_dev: &mut virtio::Device) -> Result {
/// #       Ok(())
/// #   }
///     define_virtio_id_table! {(), [
///         ({ device: 0x00000001, vendor: 0xffffffff }, None),
///     ]}
/// # }
/// ```
#[macro_export]
macro_rules! define_virtio_id_table {
    ($data_type:ty, $($t:tt)*) => {
        $crate::define_id_table!(ID_TABLE, $crate::virtio::DeviceId, $data_type, $($t)*);
    };
}
