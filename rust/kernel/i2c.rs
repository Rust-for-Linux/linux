// SPDX-License-Identifier: GPL-2.0

//! I2C devices and drivers.
//!
//! C header: [`include/linux/i2c.h`](../../../../include/linux/i2c.h)

use crate::{
    bindings,
    device::Device,
    device_id::{self, RawDeviceId},
    driver,
    error::{from_result, to_result, Result},
    of,
    str::{BStr, CStr},
    types::ForeignOwnable,
    ThisModule,
};

/// An I2C device id.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct DeviceId(bindings::i2c_device_id);

impl DeviceId {
    /// Create a new I2C DeviceId
    pub const fn new(name: &CStr) -> Self {
        let device_id = core::mem::MaybeUninit::<bindings::i2c_device_id>::zeroed();
        let mut device_id = unsafe { device_id.assume_init() };

        let name = BStr::from_bytes(name.as_bytes_with_nul());
        assert!(name.len() <= device_id.name.len());

        let mut i = 0;
        while i < name.len() {
            device_id.name[i] = name.deref_const()[i] as _;
            i += 1;
        }

        Self(device_id)
    }
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores `offset` in `i2c_device_id::driver_data`.
unsafe impl RawDeviceId for DeviceId {
    type RawType = bindings::i2c_device_id;
    const DRIVER_DATA_OFFSET: usize = core::mem::offset_of!(bindings::i2c_device_id, driver_data);
}

/// Alias for `device_id::IdTable` containing I2C's `DeviceId`
pub type IdTable<T> = &'static dyn device_id::IdTable<DeviceId, T>;

/// An adapter for the registration of i2c drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver> driver::RegistrationOps for Adapter<T> {
    type RegType = bindings::i2c_driver;

    fn register(
        i2cdrv: &mut Self::RegType,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        i2cdrv.driver.name = name.as_char_ptr();
        i2cdrv.probe = Some(Self::probe_callback);
        i2cdrv.remove = Some(Self::remove_callback);
        if let Some(t) = T::I2C_DEVICE_ID_TABLE {
            i2cdrv.id_table = t.as_ptr();
        }
        if let Some(t) = T::OF_DEVICE_ID_TABLE {
            i2cdrv.driver.of_match_table = t.as_ptr();
        }

        // SAFETY:
        //   - `pdrv` lives at least until the call to `platform_driver_unregister()` returns.
        //   - `name` pointer has static lifetime.
        //   - `module.0` lives at least as long as the module.
        //   - `probe()` and `remove()` are static functions.
        //   - `of_match_table` is either a raw pointer with static lifetime,
        //      as guaranteed by the [`device_id::IdTable`] type, or null.
        to_result(unsafe { bindings::i2c_register_driver(module.0, i2cdrv) })
    }

    fn unregister(i2cdrv: &mut Self::RegType) {
        // SAFETY: By the safety requirements of this function (defined in the trait definition),
        // `reg` was passed (and updated) by a previous successful call to
        // `i2c_register_driver`.
        unsafe { bindings::i2c_del_driver(i2cdrv) };
    }
}

impl<T: Driver> Adapter<T> {
    extern "C" fn probe_callback(i2c: *mut bindings::i2c_client) -> core::ffi::c_int {
        from_result(|| {
            let mut client = unsafe { Client::from_ptr(i2c) };
            let data = T::probe(&mut client)?;

            // SAFETY: `i2c` is guaranteed to be a valid, non-null pointer.
            unsafe { bindings::i2c_set_clientdata(i2c, data.into_foreign() as _) };
            Ok(0)
        })
    }

    extern "C" fn remove_callback(i2c: *mut bindings::i2c_client) {
        // SAFETY: `i2c` is guaranteed to be a valid, non-null pointer
        let ptr = unsafe { bindings::i2c_get_clientdata(i2c) };
        // SAFETY:
        //   - we allocated this pointer using `T::Data::into_pointer`,
        //     so it is safe to turn back into a `T::Data`.
        //   - the allocation happened in `probe`, no-one freed the memory,
        //     `remove` is the canonical kernel location to free driver data. so OK
        //     to convert the pointer back to a Rust structure here.
        let data = unsafe { T::Data::from_foreign(ptr) };
        T::remove(&data);
    }
}

/// A I2C driver.
pub trait Driver {
    /// Data stored on device by driver.
    ///
    /// Corresponds to the data set or retrieved via the kernel's
    /// `i2c_{set,get}_clientdata()` functions.
    ///
    /// Require that `Data` implements `ForeignOwnable`. We guarantee to
    /// never move the underlying wrapped data structure. This allows
    type Data: ForeignOwnable = ();

    /// The type holding information about each device id supported by the driver.
    type IdInfo: 'static = ();

    /// The table of i2c device ids supported by the driver.
    const I2C_DEVICE_ID_TABLE: Option<IdTable<Self::IdInfo>> = None;

    /// The table of OF device ids supported by the driver.
    const OF_DEVICE_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = None;

    /// I2C driver probe.
    ///
    /// Called when a new i2c client is added or discovered.
    /// Implementers should attempt to initialize the client here.
    fn probe(client: &mut Client) -> Result<Self::Data>;

    /// I2C driver remove.
    ///
    /// Called when an i2c client is removed.
    fn remove(_data: &Self::Data) {}
}

/// A I2C Client device.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Client {
    ptr: *mut bindings::i2c_client,
}

impl Client {
    /// Creates a new client from the given pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. It must remain valid for the lifetime of the returned
    /// instance.
    unsafe fn from_ptr(ptr: *mut bindings::i2c_client) -> Self {
        // INVARIANT: The safety requirements of the function ensure the lifetime invariant.
        Self { ptr }
    }

    /// Returns the raw I2C client structure.
    pub fn raw_client(&self) -> *mut bindings::i2c_client {
        self.ptr
    }
}

impl AsRef<Device> for Client {
    fn as_ref(&self) -> &Device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { Device::as_ref(&mut (*self.ptr).dev) }
    }
}

/// Declares a kernel module that exposes a single i2c driver.
///
/// # Examples
///
/// ```ignore
/// # use kernel::{i2c, define_i2c_id_table, module_i2c_driver};
/// kernel::module_i2c_id_table!(MOD_TABLE, I2C_CLIENT_I2C_ID_TABLE);
/// kernel::define_i2c_id_table! {I2C_CLIENT_I2C_ID_TABLE, (), [
///     (i2c::DeviceId(b"fpga"), None),
/// ]}
/// struct MyDriver;
/// impl i2c::Driver for MyDriver {
///     kernel::driver_i2c_id_table!(I2C_CLIENT_I2C_ID_TABLE);
///     // [...]
/// #   fn probe(_client: &mut i2c::Client) -> Result {
/// #       Ok(())
/// #   }
/// }
///
/// module_i2c_driver! {
///     type: MyDriver,
///     name: "module_name",
///     author: "Author name",
///     license: "GPL",
/// }
/// ```
#[macro_export]
macro_rules! module_i2c_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::i2c::Adapter<T>, { $($f)* });
    };
}

/// Create a I2C `IdTable` with its alias for modpost.
#[macro_export]
macro_rules! i2c_device_table {
    ($module_table_name:ident, $table_name:ident, $id_info_type: ty, $table_data: expr) => {
        const $table_name: $crate::device_id::IdArray<
            $crate::i2c::DeviceId,
            $id_info_type,
            { $table_data.len() },
        > = $crate::device_id::IdArray::new($table_data);

        $crate::module_device_table!("i2c", $module_table_name, $table_name);
    };
}
