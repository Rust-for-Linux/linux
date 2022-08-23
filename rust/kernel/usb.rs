// SPDX-License-Identifier: GPL-2.0

//! USB devices and drivers.
//!
//! C header: [`include/linux/usb.h`](../../../../include/linux/usb.h)

use core::num::NonZeroU64;

use crate::{
    bindings, container_of,
    device::{self, RawDevice},
    driver,
    error::{code::*, from_kernel_result, Result},
    file::IoctlCommand,
    macros::vtable,
    power::PmMessage,
    str::CStr,
    to_result,
    types::ForeignOwnable,
    ThisModule,
};

/// USB device ID macros reexports and casted to [`u16`] intended for the
/// [`match_flags`](DeviceId::match_flags) field of [`DeviceId`].
pub mod id_match {
    #![allow(missing_docs)]
    pub const USB_DEVICE_ID_MATCH_VENDOR: u16 = bindings::USB_DEVICE_ID_MATCH_VENDOR as u16;
    pub const USB_DEVICE_ID_MATCH_PRODUCT: u16 = bindings::USB_DEVICE_ID_MATCH_PRODUCT as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_LO: u16 = bindings::USB_DEVICE_ID_MATCH_DEV_LO as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_HI: u16 = bindings::USB_DEVICE_ID_MATCH_DEV_HI as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_CLASS: u16 = bindings::USB_DEVICE_ID_MATCH_DEV_CLASS as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_SUBCLASS: u16 =
        bindings::USB_DEVICE_ID_MATCH_DEV_SUBCLASS as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_PROTOCOL: u16 =
        bindings::USB_DEVICE_ID_MATCH_DEV_PROTOCOL as u16;
    pub const USB_DEVICE_ID_MATCH_INT_CLASS: u16 = bindings::USB_DEVICE_ID_MATCH_INT_CLASS as u16;
    pub const USB_DEVICE_ID_MATCH_INT_SUBCLASS: u16 =
        bindings::USB_DEVICE_ID_MATCH_INT_SUBCLASS as u16;
    pub const USB_DEVICE_ID_MATCH_INT_PROTOCOL: u16 =
        bindings::USB_DEVICE_ID_MATCH_INT_PROTOCOL as u16;
    pub const USB_DEVICE_ID_MATCH_INT_NUMBER: u16 = bindings::USB_DEVICE_ID_MATCH_INT_NUMBER as u16;
    pub const USB_DEVICE_ID_MATCH_DEVICE: u16 = bindings::USB_DEVICE_ID_MATCH_DEVICE as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_RANGE: u16 = bindings::USB_DEVICE_ID_MATCH_DEV_RANGE as u16;
    pub const USB_DEVICE_ID_MATCH_DEVICE_AND_VERSION: u16 =
        bindings::USB_DEVICE_ID_MATCH_DEVICE_AND_VERSION as u16;
    pub const USB_DEVICE_ID_MATCH_DEV_INFO: u16 = bindings::USB_DEVICE_ID_MATCH_DEV_INFO as u16;
    pub const USB_DEVICE_ID_MATCH_INT_INFO: u16 = bindings::USB_DEVICE_ID_MATCH_INT_INFO as u16;
}

use id_match::*;

/// Check if USB is disabled.
pub fn disabled() -> bool {
    // SAFETY: FFI call.
    unsafe { bindings::usb_disabled() != 0 }
}

/// Registration of an USB interface driver.
pub type Registration<T> = driver::Registration<Adapter<T>>;

/// An adapter for the registration of USB interface drivers.
pub struct Adapter<T: Driver>(T);

impl<T: Driver> driver::DriverOps for Adapter<T> {
    type RegType = bindings::usb_driver;

    unsafe fn register(
        reg: *mut bindings::usb_driver,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        // SAFETY: the driver to register `reg` points to valid, initialized and writable memory.
        let pdrv = unsafe { &mut *reg };

        pdrv.name = name.as_char_ptr();
        pdrv.probe = Some(Self::probe_callback);
        pdrv.disconnect = Some(Self::disconnect_callback);
        if let Some(t) = T::ID_TABLE {
            pdrv.id_table = t.as_ref();
        }
        if T::HAS_IOCTL {
            pdrv.unlocked_ioctl = Some(Self::unlocked_ioctl_callback);
        }
        if T::HAS_SUSPEND {
            pdrv.suspend = Some(Self::suspend_callback);
        }
        if T::HAS_RESUME {
            pdrv.resume = Some(Self::resume_callback);
        }
        if T::HAS_RESET_RESUME {
            pdrv.reset_resume = Some(Self::reset_resume_callback);
        }
        if T::HAS_PRE_RESET {
            pdrv.pre_reset = Some(Self::pre_reset_callback);
        }
        if T::HAS_POST_RESET {
            pdrv.post_reset = Some(Self::post_reset_callback);
        }
        if let Some(t) = T::ID_TABLE {
            pdrv.id_table = t.as_ref();
        }
        // SAFETY: `reg`, `module.0` and `name.as_char_ptr()` all point to valid data.
        to_result(unsafe { bindings::usb_register_driver(reg, module.0, name.as_char_ptr()) })
    }

    unsafe fn unregister(reg: *mut bindings::usb_driver) {
        // SAFETY: the driver to unregister `reg` points to valid, initialized and writable memory.
        unsafe { bindings::usb_deregister(reg) }
    }
}

impl<T: Driver> Adapter<T> {
    extern "C" fn probe_callback(
        intf: *mut bindings::usb_interface,
        id: *const bindings::usb_device_id,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // SAFETY: `intf` is always a valid pointer passed from the caller.
            let mut dev = unsafe { Interface::from_ptr(intf) };
            // SAFETY: `id` is a pointer within the static table, so it's always valid.
            let info = unsafe {
                NonZeroU64::new((*id).driver_info).map(|o| &*(id.cast::<u8>().offset(o.get() as _).cast::<T::IdInfo>()))
            };
            let data = T::probe(&mut dev, info)?;
            let ptr = T::Data::into_foreign(data);
            // SAFETY: `ptr` must either be null or valid.
            unsafe { bindings::usb_set_intfdata(intf, ptr as _) };
            Ok(0)
        }
    }

    extern "C" fn disconnect_callback(intf: *mut bindings::usb_interface) {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let ptr = unsafe { bindings::usb_get_intfdata(intf) };
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        // SAFETY: `ptr` can be null or must be valid.
        let data = unsafe { T::Data::from_foreign(ptr) };
        T::disconnect(&mut intf, &data);
        <T::Data as driver::DeviceRemoval>::device_remove(&data);
        // SAFETY: passing null clears the interface data.
        unsafe { bindings::usb_set_intfdata(intf.ptr, core::ptr::null_mut()) };
    }

    extern "C" fn unlocked_ioctl_callback(
        intf: *mut bindings::usb_interface,
        code: u32,
        buf: *mut core::ffi::c_void,
    ) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        let mut cmd = IoctlCommand::new(code, buf as usize);
        from_kernel_result! {
            let res = T::ioctl(&mut intf, &mut cmd)?;
            Ok(res)
        }
    }

    extern "C" fn suspend_callback(intf: *mut bindings::usb_interface, message: PmMessage) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        from_kernel_result! {
            T::suspend(&mut intf, message)?;
            Ok(0)
        }
    }

    extern "C" fn resume_callback(intf: *mut bindings::usb_interface) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        from_kernel_result! {
            T::resume(&mut intf)?;
            Ok(0)
        }
    }

    extern "C" fn reset_resume_callback(intf: *mut bindings::usb_interface) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        from_kernel_result! {
            T::reset_resume(&mut intf)?;
            Ok(0)
        }
    }

    extern "C" fn pre_reset_callback(intf: *mut bindings::usb_interface) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        from_kernel_result! {
            T::pre_reset(&mut intf)?;
            Ok(0)
        }
    }

    extern "C" fn post_reset_callback(intf: *mut bindings::usb_interface) -> i32 {
        // SAFETY: `intf` is always a valid pointer passed from the caller.
        let mut intf = unsafe { Interface::from_ptr(intf) };
        from_kernel_result! {
            T::post_reset(&mut intf)?;
            Ok(0)
        }
    }
}

/// Device table entry for table-driven USB drivers.
#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct DeviceId {
    /// Mask used to match against new devices.
    pub match_flags: u16,
    /// USB vendor ID for a device.
    pub id_vendor: u16,
    /// Vendor-assigned product ID.
    pub id_product: u16,
    /// Low end of range of vendor-assigned product version numbers.
    pub bcd_device_lo: u16,
    /// High end of version number range.
    pub bcd_device_hi: u16,
    /// Class of device.
    pub b_device_class: u8,
    /// Subclass of device.
    pub b_device_subclass: u8,
    /// Protocol of device.
    pub b_device_protocol: u8,
    /// Class of interface.
    pub b_interface_class: u8,
    /// Subclass of interface.
    pub b_interface_subclass: u8,
    /// Protocol of interface.
    pub b_interface_protocol: u8,
    /// Number of interface.
    pub b_interface_number: u8,
}

impl DeviceId {
    /// `USB_DEVICE` macro.
    pub const fn new(id_vendor: u16, id_product: u16) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEVICE,
            id_vendor,
            id_product,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_VER` macro.
    pub const fn with_version(
        id_vendor: u16,
        id_product: u16,
        bcd_device_lo: u16,
        bcd_device_hi: u16,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEVICE_AND_VERSION,
            id_vendor,
            id_product,
            bcd_device_lo,
            bcd_device_hi,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_INTERFACE_CLASS` macro.
    pub const fn with_interface_class(
        id_vendor: u16,
        id_product: u16,
        b_interface_class: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEVICE | USB_DEVICE_ID_MATCH_INT_CLASS,
            id_vendor,
            id_product,
            b_interface_class,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_INTERFACE_PROTOCOL` macro.
    pub const fn with_interface_protocol(
        id_vendor: u16,
        id_product: u16,
        b_interface_protocol: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEVICE | USB_DEVICE_ID_MATCH_INT_PROTOCOL,
            id_vendor,
            id_product,
            b_interface_protocol,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_INTERFACE_NUMBER` macro.
    pub const fn with_interface_number(
        id_vendor: u16,
        id_product: u16,
        b_interface_number: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEVICE | USB_DEVICE_ID_MATCH_INT_NUMBER,
            id_vendor,
            id_product,
            b_interface_number,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_INFO` macro.
    pub const fn with_info(
        b_device_class: u8,
        b_device_subclass: u8,
        b_device_protocol: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_DEV_INFO,
            b_device_class,
            b_device_subclass,
            b_device_protocol,
            ..Self::default()
        }
    }

    /// `USB_INTERFACE_INFO` macro.
    pub const fn with_interface_info(
        b_interface_class: u8,
        b_interface_subclass: u8,
        b_interface_protocol: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_INT_INFO,
            b_interface_class,
            b_interface_subclass,
            b_interface_protocol,
            ..Self::default()
        }
    }

    /// `USB_DEVICE_AND_INTERFACE_INFO` macro.
    pub const fn with_device_and_interface_info(
        id_vendor: u16,
        id_product: u16,
        b_interface_class: u8,
        b_interface_subclass: u8,
        b_interface_protocol: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_INT_INFO | USB_DEVICE_ID_MATCH_DEVICE,
            id_vendor,
            id_product,
            b_interface_class,
            b_interface_subclass,
            b_interface_protocol,
            ..Self::default()
        }
    }

    /// `USB_VENDOR_AND_INTERFACE_INFO` macro.
    pub const fn with_vendor_and_interface_info(
        id_vendor: u16,
        b_interface_class: u8,
        b_interface_subclass: u8,
        b_interface_protocol: u8,
    ) -> Self {
        Self {
            match_flags: USB_DEVICE_ID_MATCH_INT_INFO | USB_DEVICE_ID_MATCH_VENDOR,
            id_vendor,
            b_interface_class,
            b_interface_subclass,
            b_interface_protocol,
            ..Self::default()
        }
    }

    /// Constructor that sets every field to `0`.
    pub const fn default() -> Self {
        Self {
            match_flags: 0,
            id_vendor: 0,
            id_product: 0,
            bcd_device_lo: 0,
            bcd_device_hi: 0,
            b_device_class: 0,
            b_device_subclass: 0,
            b_device_protocol: 0,
            b_interface_class: 0,
            b_interface_subclass: 0,
            b_interface_protocol: 0,
            b_interface_number: 0,
        }
    }
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores `info` in `usb_device_id::driver_info`.
unsafe impl const driver::RawDeviceId for DeviceId {
    type RawType = bindings::usb_device_id;

    const ZERO: Self::RawType = bindings::usb_device_id {
        match_flags: 0,
        idVendor: 0,
        idProduct: 0,
        bcdDevice_lo: 0,
        bcdDevice_hi: 0,
        bDeviceClass: 0,
        bDeviceSubClass: 0,
        bDeviceProtocol: 0,
        bInterfaceClass: 0,
        bInterfaceSubClass: 0,
        bInterfaceProtocol: 0,
        bInterfaceNumber: 0,
        driver_info: 0,
    };

    fn to_rawid(&self, info: isize) -> Self::RawType {
        bindings::usb_device_id {
            match_flags: self.match_flags,
            idVendor: self.id_vendor,
            idProduct: self.id_product,
            bcdDevice_lo: self.bcd_device_lo,
            bcdDevice_hi: self.bcd_device_hi,
            bDeviceClass: self.b_device_class,
            bDeviceSubClass: self.b_device_subclass,
            bDeviceProtocol: self.b_device_protocol,
            bInterfaceClass: self.b_interface_class,
            bInterfaceSubClass: self.b_interface_subclass,
            bInterfaceProtocol: self.b_interface_protocol,
            bInterfaceNumber: self.b_interface_number,
            driver_info: info as _,
        }
    }
}

/// Define a const USB device id table.
///
/// # Examples
///
/// ```
/// use kernel::{usb, define_usb_id_table};
///
/// struct MyDriver;
/// impl usb::Driver for MyDriver {
///     // [...]
///     fn probe(_dev: &mut usb::Interface, _id_info: Option<&Self::IdInfo>) -> Result {
///         Ok(())
///     }
///     define_usb_id_table! {u64, [
///         (usb::DeviceId::new(0xbee7, 0xffff), None),
///         (usb::DeviceId::with_class(0xbee7, 0xff1d), Some(0x40)),
///     ]}
/// }
/// ```
#[macro_export]
macro_rules! define_usb_id_table {
    ($data_type:ty, $($t:tt)*) => {
        type IdInfo = $data_type;
        $crate::define_id_table!(ID_TABLE, $crate::usb::DeviceId, $data_type, $($t)*);
    };
}

/// Declares a kernel module that exposes a single USB driver.
///
/// # Examples
///
/// ```ignore
/// use kernel::{usb, define_usb_id_table, module_usb_driver};
///
/// struct MyDriver;
/// impl usb::Driver for MyDriver {
///     // [...]
///     fn probe(_dev: &mut usb::Device, _id: Option<&Self::IdInfo>) -> Result {
///         Ok(())
///     }
///     define_usb_id_table! {(), [
///         ({ id: 0xfff0, mask: 0xfff0 }, None),
///     ]}
/// }
///
/// module_usb_driver! {
///     type: MyDriver,
///     name: "module_name",
///     author: "Author name",
///     license: "GPL",
/// }
/// ```
#[macro_export]
macro_rules! module_usb_driver {
    ($($f:tt)*) => {
        $crate::module_driver!(<T>, $crate::usb::Adapter<T>, { $($f)* });
    };
}

/// An USB interface driver.
#[vtable]
pub trait Driver {
    /// Data stored on device interface by driver.
    ///
    /// Corresponds to the data set or retrieved via the kernel's
    /// `usb_{set,get}_intfdata()` functions.
    ///
    /// Require that `Data` implements `ForeignOwnable`. We guarantee to
    /// never move the underlying wrapped data structure.
    type Data: ForeignOwnable + Send + Sync + driver::DeviceRemoval = ();

    /// The type holding information about each device id supported by the driver.
    type IdInfo: 'static = ();

    /// The table of device ids supported by the driver.
    const ID_TABLE: Option<driver::IdTable<'static, DeviceId, Self::IdInfo>> = None;

    /// USB driver probe.
    ///
    /// Called to see if the driver can manage a device interface.
    /// Implementer should attempt to initialize the interface here.
    fn probe(dev: &mut Interface, id: Option<&Self::IdInfo>) -> Result<Self::Data>;

    /// USB driver disconnect.
    ///
    /// Called when a device interface is removed.
    /// Implementer should prepare the interface for removal here.
    fn disconnect(_intf: &mut Interface, _data: &Self::Data) {}

    /// USB driver ioctl.
    ///
    /// Used for drivers that want to talk to userspace through the `usbfs` filesystem.
    fn ioctl(_intf: &mut Interface, _cmd: &mut IoctlCommand) -> Result<i32> {
        Err(EINVAL)
    }

    /// USB driver suspend.
    ///
    /// Called when the device is going to be suspended by the system either from system sleep or
    /// runtime suspend context. The return value will be ignored in system sleep context, so do
    /// NOT try to continue using the device if suspend fails in this case. Instead, let the resume
    /// or reset-resume routine recover from the failure.
    fn suspend(_intf: &mut Interface, _message: PmMessage) -> Result {
        Ok(())
    }

    /// USB driver resume.
    ///
    /// Called when the device is being resumed by the system.
    fn resume(_intf: &mut Interface) -> Result {
        Ok(())
    }

    /// USB driver reset-resume.
    ///
    /// Called when the suspended device has been reset instead of being resumed.
    fn reset_resume(_intf: &mut Interface) -> Result {
        Ok(())
    }

    /// USB driver pre-reset.
    ///
    /// Called by `usb_reset_device()` when the device is about to be reset. This routine must not
    /// return until the driver has no active URBs for the device, and no more URBs may be
    /// submitted until the [`post_reset`] method is called.
    fn pre_reset(_intf: &mut Interface) -> Result {
        Ok(())
    }

    /// USB driver post-reset.
    ///
    /// Called by `usb_reset_device()` after the device has been reset.
    fn post_reset(_intf: &mut Interface) -> Result {
        Ok(())
    }
}

/// An USB device.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Device {
    ptr: *mut bindings::usb_device,
}

impl Device {
    /// Creates a device from a raw pointer.
    ///
    /// # Safety
    ///
    /// It requires that a non-null and valid pointer to [`bindings::usb_device`] has to be passed.
    /// Must remain as valid during the life time of the instance as well.
    #[inline]
    unsafe fn from_ptr(ptr: *mut bindings::usb_device) -> Self {
        Self { ptr }
    }

    /// Returns the raw `struct usb_device` related to `self`.
    #[inline]
    pub fn raw(&self) -> *mut bindings::usb_device {
        self.ptr
    }

    /// Gets the generic interface of this device.
    #[inline]
    pub fn to_device(&self) -> device::Device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { device::Device::new(self.raw_device()) }
    }

    /// Increments the reference count of the device.
    ///
    /// # Safety
    ///
    /// The device must not be released prior to calling this function.
    #[inline]
    pub fn get(&mut self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            self.ptr = bindings::usb_get_dev(self.ptr);
        }
    }

    /// Decrements the reference count of the device.
    ///
    /// # Safety
    ///
    /// The reference count must be greater than zero.
    #[inline]
    pub fn put(&self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { bindings::usb_put_dev(self.ptr) }
    }

    #[inline]
    fn create_pipe(&self, endpoint: u32) -> u32 {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { (((*self.ptr).devnum as u32) << 8) | (endpoint << 15) }
    }

    /// Gets the send control pipe of an endpoint.
    #[inline]
    pub fn sndctrlpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_CONTROL << 30) | self.create_pipe(endpoint)
    }

    /// Gets the receive control pipe of an endpoint.
    #[inline]
    pub fn rcvctrlpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_CONTROL << 30) | self.create_pipe(endpoint) | bindings::USB_DIR_IN
    }

    /// Gets the send isochronous pipe of an endpoint.
    #[inline]
    pub fn sndisocpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_ISOCHRONOUS << 30) | self.create_pipe(endpoint)
    }

    /// Gets the receive isochronous pipe of an endpoint.
    #[inline]
    pub fn rcvisocpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_ISOCHRONOUS << 30) | self.create_pipe(endpoint) | bindings::USB_DIR_IN
    }

    /// Gets the send bulk pipe of an endpoint.
    #[inline]
    pub fn sndbulkpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_BULK << 30) | self.create_pipe(endpoint)
    }

    /// Gets the receive bulk pipe of an endpoint.
    #[inline]
    pub fn rcvbulkpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_BULK << 30) | self.create_pipe(endpoint) | bindings::USB_DIR_IN
    }

    /// Gets the send interrupt pipe of an endpoint.
    #[inline]
    pub fn sndintpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_INTERRUPT << 30) | self.create_pipe(endpoint)
    }

    /// Gets the receive interrupt pipe of an endpoint.
    #[inline]
    pub fn rcvintpipe(&self, endpoint: u32) -> u32 {
        (bindings::PIPE_INTERRUPT << 30) | self.create_pipe(endpoint) | bindings::USB_DIR_IN
    }
}

// SAFETY: The device returned by `raw_device` is the raw USB device.
unsafe impl device::RawDevice for Device {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.ptr).dev }
    }
}

// SAFETY: `Device` only holds a pointer to an USB device, which is safe to be used from any thread.
unsafe impl Send for Device {}

// SAFETY: `Device` only holds a pointer to an USB device, references to which are safe to be used
// from any thread.
unsafe impl Sync for Device {}

/// An USB device interface.
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Interface {
    ptr: *mut bindings::usb_interface,
}

impl Interface {
    /// Creates an interface from a raw pointer.
    ///
    /// # Safety
    ///
    /// It requires that a valid pointer to [`bindings::usb_interface`] has to be passed.
    /// Must remain as valid during the life time of the instance as well.
    #[inline]
    unsafe fn from_ptr(ptr: *mut bindings::usb_interface) -> Self {
        Self { ptr }
    }

    /// Returns the raw `struct usb_interface` related to `self`.
    pub fn raw(&self) -> *mut bindings::usb_interface {
        self.ptr
    }

    /// Gets the generic interface of this device.
    #[inline]
    pub fn to_device(&self) -> device::Device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { device::Device::new(self.raw_device()) }
    }

    /// Returns the USB hub with its lifetime managed by the interface.
    #[inline]
    pub fn to_usb_device(&self) -> Device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            Device::from_ptr(
                container_of!((*self.ptr).dev.parent, bindings::usb_device, dev).cast_mut(),
            )
        }
    }

    /// Increments the reference count of the interface.
    ///
    /// # Safety
    ///
    /// The interface must not be released prior to calling this function.
    #[inline]
    pub fn get(&mut self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            self.ptr = bindings::usb_get_intf(self.ptr);
        }
    }

    /// Decrements the reference count of the interface.
    ///
    /// # Safety
    ///
    /// The reference count must be greater than zero.
    #[inline]
    pub fn put(&self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { bindings::usb_put_intf(self.ptr) }
    }
}

// SAFETY: The device returned by `raw_device` is the raw USB interface.
unsafe impl device::RawDevice for Interface {
    fn raw_device(&self) -> *mut bindings::device {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { &mut (*self.ptr).dev }
    }
}

// SAFETY: `Interface` only holds a pointer to an USB interface, which is safe to be used from any
// thread.
unsafe impl Send for Interface {}

// SAFETY: `Interface` only holds a pointer to an USB interface, references to which are safe to be
// used from any thread.
unsafe impl Sync for Interface {}
