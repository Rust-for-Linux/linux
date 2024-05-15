// SPDX-License-Identifier: GPL-2.0

//! This module provides safer and higher level abstraction over the kernel's SPI types
//! and functions.
//!
//! C header: [`include/linux/spi/spi.h`](../../../../include/linux/spi/spi.h)

use crate::bindings;
use crate::c_str;
use crate::error::{code::*, from_result, Error, Result};
use crate::static_assert;
use crate::str::CStr;
use alloc::boxed::Box;
use core::marker::PhantomData;
use core::pin::Pin;
use macros::vtable;

/// Wrapper struct around the kernel's `spi_device`.
#[derive(Clone, Copy)]
pub struct SpiDevice(*mut bindings::spi_device);

impl SpiDevice {
    /// Create an [`SpiDevice`] from a mutable spi_device raw pointer. This function is unsafe
    /// as the pointer might be invalid.
    ///
    /// The pointer must be valid. This can be achieved by calling `to_ptr` on a previously
    /// constructed, safe `SpiDevice` instance, or by making sure that the pointer points
    /// to valid memory.
    ///
    /// You probably do not want to use this abstraction directly. It is mainly used
    /// by this abstraction to wrap valid pointers given by the Kernel to the different
    /// SPI methods: `probe`, `remove` and `shutdown`.
    pub unsafe fn from_ptr(dev: *mut bindings::spi_device) -> Self {
        SpiDevice(dev)
    }

    /// Access the raw pointer from an [`SpiDevice`] instance.
    pub fn to_ptr(&mut self) -> *mut bindings::spi_device {
        self.0
    }
}

/// Corresponds to the kernel's spi_driver's methods. Implement this trait on a type to
/// express the need of a custom probe, remove or shutdown function for your SPI driver.
#[vtable]
pub trait SpiMethods {
    /// Corresponds to the kernel's `spi_driver`'s `probe` method field.
    fn probe(_spi_dev: &mut SpiDevice) -> Result<i32> {
        unreachable!("There should be a NULL in the probe filed of spi_driver's");
    }

    /// Corresponds to the kernel's `spi_driver`'s `remove` method field.
    fn remove(_spi_dev: &mut SpiDevice) {
        unreachable!("There should be a NULL in the remove filed of spi_driver's");
    }

    /// Corresponds to the kernel's `spi_driver`'s `shutdown` method field.
    fn shutdown(_spi_dev: &mut SpiDevice) {
        unreachable!("There should be a NULL in the shutdown filed of spi_driver's");
    }
}

/// Registration of an SPI driver.
pub struct DriverRegistration<T: SpiMethods> {
    this_module: &'static crate::ThisModule,
    registered: bool,
    name: &'static CStr,
    id_table: Option<&'static [SpiDeviceId]>,
    spi_driver: bindings::spi_driver,
    _p: PhantomData<T>,
}

impl<T: SpiMethods> DriverRegistration<T> {
    fn new(
        this_module: &'static crate::ThisModule,
        name: &'static CStr,
        id_table: Option<&'static [SpiDeviceId]>,
    ) -> Self {
        DriverRegistration {
            this_module,
            name,
            registered: false,
            id_table,
            spi_driver: bindings::spi_driver::default(),
            _p: PhantomData,
        }
    }

    /// Create a new `DriverRegistration` and register it. This is equivalent to creating
    /// a static `spi_driver` and then calling `spi_driver_register` on it in C.
    ///
    /// # Examples
    ///
    /// ```
    /// let spi_driver =
    ///     spi::DriverRegistration::new_pinned::<MySpiMethods>(&THIS_MODULE, cstr!("my_driver_name"))?;
    /// ```
    pub fn new_pinned(
        this_module: &'static crate::ThisModule,
        name: &'static CStr,
        id_table: Option<&'static [SpiDeviceId]>,
    ) -> Result<Pin<Box<Self>>> {
        let mut registration = Pin::from(Box::try_new(Self::new(this_module, name, id_table))?);

        registration.as_mut().register()?;

        Ok(registration)
    }

    /// Register a [`DriverRegistration`]. This is equivalent to calling `spi_driver_register`
    /// on your `spi_driver` in C, without creating it first.
    fn register(self: Pin<&mut Self>) -> Result {
        // SAFETY: We do not move out of the reference we get, and are only registering
        // `this` once over the course of the module, since we check that the `registered`
        // field was not already set to true.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            return Err(EINVAL);
        }

        let mut spi_driver = this.spi_driver;

        if let Some(id_table) = this.id_table {
            spi_driver.id_table = id_table.as_ptr() as *const bindings::spi_device_id;
        }

        if T::HAS_PROBE {
            spi_driver.probe = Some(probe_callback::<T>);
        }
        if T::HAS_REMOVE {
            spi_driver.remove = Some(remove_callback::<T>);
        }
        if T::HAS_SHUTDOWN {
            spi_driver.remove = Some(shutdown_callback::<T>);
        }

        spi_driver.driver.name = this.name.as_ptr() as *const core::ffi::c_char;

        // SAFETY: Since we are using a pinned `self`, we can register the driver safely as
        // if we were using a static instance. The kernel will access this driver over the
        // entire lifespan of a module and therefore needs a pointer valid for the entirety
        // of this lifetime.
        let res =
            unsafe { bindings::__spi_register_driver(this.this_module.0, &mut this.spi_driver) };

        if res != 0 {
            return Err(Error::from_errno(res));
        }

        this.registered = true;

        Ok(())
    }
}

impl<T: SpiMethods> Drop for DriverRegistration<T> {
    fn drop(&mut self) {
        // SAFETY: We are simply unregistering an `spi_driver` that we know to be valid.
        // [`DriverRegistration`] instances can only be created by being registered at the
        // same time, so we are sure that we'll never unregister an unregistered `spi_driver`.
        unsafe { bindings::driver_unregister(&mut self.spi_driver.driver) }
    }
}

unsafe extern "C" fn probe_callback<T: SpiMethods>(
    spi: *mut bindings::spi_device,
) -> core::ffi::c_int {
    from_result(|| Ok(T::probe(&mut SpiDevice(spi))?))
}

unsafe extern "C" fn remove_callback<T: SpiMethods>(spi: *mut bindings::spi_device) {
    T::remove(&mut SpiDevice(spi));
}

unsafe extern "C" fn shutdown_callback<T: SpiMethods>(spi: *mut bindings::spi_device) {
    T::shutdown(&mut SpiDevice(spi));
}

// SAFETY: The only method is `register()`, which requires a (pinned) mutable `Registration`, so it
// is safe to pass `&Registration` to multiple threads because it offers no interior mutability.
unsafe impl<T: SpiMethods> Sync for DriverRegistration<T> {}

// SAFETY: All functions work from any thread.
unsafe impl<T: SpiMethods> Send for DriverRegistration<T> {}

/// We need a union because we can't get an address from a pointer at compile time.
#[repr(C)]
union DriverData {
    ptr: &'static (),
    val: usize,
}

/// Wrapper struct around the kernel's `struct spi_device_id`.
#[repr(C)]
pub struct SpiDeviceId {
    name: [i8; 32],
    driver_data: DriverData,
}

static_assert!(
    core::mem::size_of::<SpiDeviceId>() == core::mem::size_of::<bindings::spi_device_id>()
);

impl SpiDeviceId {
    /// Creates a new [SpiDeviceId] with a given name.
    /// <p style="background:rgba(255,181,77,0.16);padding:0.75em;">
    /// <strong>Warning:</strong> The name will be truncated to a maximum of 32 chars
    /// </p>
    ///
    /// To specify `driver_data` content, see [SpiDeviceId::with_driver_data_pointer] and
    /// [SpiDeviceId::with_driver_data_number].
    pub const fn new(name: &CStr) -> Self {
        let name = name.as_bytes_with_nul();
        let name_len = name.len();

        let mut array = [0; 32];
        let min_len = if name_len > 32 { 32 } else { name_len };

        let mut i = 0;
        while i < min_len {
            array[i] = name[i] as i8;
            i += 1;
        }

        SpiDeviceId {
            name: array,
            driver_data: DriverData { val: 0 },
        }
    }

    /// Add a pointer to the `driver_data` field.
    pub const fn with_driver_data_pointer<T>(mut self, driver_data: &'static T) -> Self {
        // SAFETY: On the C side this will only be used as an integer, we don't care about the
        // type. This function is called with a reference so the deref is safe as the pointer is
        // valid
        self.driver_data = DriverData {
            ptr: unsafe { &*(driver_data as *const T as *const ()) },
        };

        // unsafe { core::mem::transmute::<&T, bindings::kernel_ulong_t>(driver_data) };
        self
    }

    /// Add a number to the `driver_data` field.
    pub const fn with_driver_data_number(mut self, driver_data: usize) -> Self {
        self.driver_data = DriverData { val: driver_data };

        self
    }

    /// Used for creating a sentinel to place at the end of an array of [SpiDeviceId].
    pub const fn sentinel() -> Self {
        // TODO: call Default trait instead when #![feature(const_trait_impl)] is stabilized
        Self::new(c_str!(""))
    }
}

/// Wrapper struct around the kernel's `struct spi_transfer`.
pub struct SpiTransfer(bindings::spi_transfer);

impl SpiTransfer {
    pub fn new() -> Self {
        SpiTransfer(bindings::spi_transfer::default())
    }

    pub fn with_tx_buf(mut self, buf: &[u8]) -> Self {
        // Make sure no tx_buf nor rx_buf has been provided before
        // TODO: replace with a `Result`
        assert!(self.0.len == 0);

        self.0.tx_buf = buf.as_ptr() as *const core::ffi::c_void;
        self.0.len = buf.len() as core::ffi::c_uint;

        self
    }

    pub fn with_rx_buf(mut self, buf: &mut[u8]) -> Self {
        // Make sure no tx_buf nor rx_buf has been provided before
        // TODO: replace with a `Result`
        assert!(self.0.len == 0);

        self.0.rx_buf = buf.as_mut_ptr() as *mut core::ffi::c_void;
        self.0.len = buf.len() as core::ffi::c_uint;

        self
    }
}

#[derive(Default)]
pub struct SpiMessage(bindings::spi_message);

impl SpiMessage {
    pub fn new() -> Self {
        Self::default()
    }
    /// Equivalent of `spi_message_init`
    pub fn spi_message_init(&mut self) {
        unsafe { bindings::spi_message_init(&mut self.0) }
    }
}

/// High level abstraction over the kernel's SPI functions such as `spi_write_then_read`.
// TODO this should be a mod, right?
pub struct Spi;

impl Spi {
    /// Corresponds to the kernel's `spi_write_then_read`.
    ///
    /// # Examples
    ///
    /// ```
    /// let to_write = "rust-for-linux".as_bytes();
    /// let mut to_receive = [0u8; 10]; // let's receive 10 bytes back
    ///
    /// // `spi_device` was previously provided by the kernel in that case
    /// let transfer_result = Spi::write_then_read(spi_device, &to_write, &mut to_receive);
    /// ```
    pub fn write_then_read(dev: &mut SpiDevice, tx_buf: &[u8], rx_buf: &mut [u8]) -> Result {
        // SAFETY: The `dev` argument must uphold the safety guarantees made when creating
        // the [`SpiDevice`] instance. It should therefore point to a valid `spi_device`
        // and valid memory. We also know that a rust slice will always contain a proper
        // size and that it is safe to use as is. Converting from a Rust pointer to a
        // generic C `void*` pointer is normal, and does not pose size issues on the
        // kernel's side, which will use the given Transfer Receive sizes as bytes.
        let res = unsafe {
            bindings::spi_write_then_read(
                dev.to_ptr(),
                tx_buf.as_ptr() as *const core::ffi::c_void,
                tx_buf.len() as core::ffi::c_uint,
                rx_buf.as_mut_ptr() as *mut core::ffi::c_void,
                rx_buf.len() as core::ffi::c_uint,
            )
        };

        match res {
            0 => Ok(()),                        // 0 indicates a valid transfer,
            err => Err(Error::from_errno(err)), // A negative number indicates an error
        }
    }

    /// Corresponds to the kernel's `spi_write`.
    ///
    /// # Examples
    ///
    /// ```
    /// let to_write = "rust-for-linux".as_bytes();
    ///
    /// // `spi_device` was previously provided by the kernel in that case
    /// let write_result = Spi::write(spi_device, &to_write);
    /// ```
    pub fn write(dev: &mut SpiDevice, tx_buf: &[u8]) -> Result {
        Spi::write_then_read(dev, tx_buf, &mut [0u8; 0])
    }

    /// Corresponds to the kernel's `spi_read`.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut to_receive = [0u8; 10]; // let's receive 10 bytes
    ///
    /// // `spi_device` was previously provided by the kernel in that case
    /// let transfer_result = Spi::read(spi_device, &mut to_receive);
    /// ```
    pub fn read(dev: &mut SpiDevice, rx_buf: &mut [u8]) -> Result {
        Spi::write_then_read(dev, &[0u8; 0], rx_buf)
    }
}
