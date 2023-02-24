// SPDX-License-Identifier: GPL-2.0

use crate::bindings;
use crate::error::{to_result, Result};

#[cfg(doc)]
use crate::error::code::ENXIO;

/// An USB endpoint descriptor.
#[derive(Default, Copy, Clone, PartialEq)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    /// Size of descriptor.
    pub b_length: u8,
    /// Descriptor type.
    pub b_descriptor_type: u8,
    /// Address of the endpoint on the USB device described by this descriptor.
    pub b_endpoint_address: u8,
    /// Endpoint attribute when configured through bConfigurationValue.
    pub bm_attributes: u8,
    /// Maximum packet size of this endpoint.
    pub w_max_packet_size: bindings::__le16,
    /// Interval for polling endpoint for data transfers.
    pub b_interval: u8,
    /// Rate at which synchronization feedback is provide, only in audio endpoints.
    pub b_refresh: u8,
    /// Address of the synchronization endpoint, only in audio endpoints.
    pub b_synch_address: u8,
}

impl EndpointDescriptor {
    /// Get the endpoint's number (`0..=15`).
    #[inline]
    pub const fn num(&self) -> u8 {
        self.b_endpoint_address & bindings::USB_ENDPOINT_NUMBER_MASK as u8
    }

    /// Get the endpoint's transfer type.
    #[inline]
    pub const fn xfer_type(&self) -> u8 {
        self.bm_attributes & bindings::USB_ENDPOINT_XFERTYPE_MASK as u8
    }

    /// Check if the endpoint has IN direction.
    #[inline]
    pub const fn dir_in(&self) -> bool {
        (self.b_endpoint_address & bindings::USB_ENDPOINT_DIR_MASK as u8)
            == bindings::USB_DIR_IN as u8
    }

    /// Check if the endpoint has OUT direction.
    #[inline]
    pub const fn dir_out(&self) -> bool {
        (self.b_endpoint_address & bindings::USB_ENDPOINT_DIR_MASK as u8)
            == bindings::USB_DIR_OUT as u8
    }

    /// Check if the endpoint has bulk transfer type.
    #[inline]
    pub const fn xfer_bulk(&self) -> bool {
        (self.bm_attributes & bindings::USB_ENDPOINT_XFERTYPE_MASK as u8)
            == bindings::USB_ENDPOINT_XFER_BULK as u8
    }

    /// Check if the endpoint has control transfer type.
    #[inline]
    pub const fn xfer_control(&self) -> bool {
        (self.bm_attributes & bindings::USB_ENDPOINT_XFERTYPE_MASK as u8)
            == bindings::USB_ENDPOINT_XFER_CONTROL as u8
    }

    /// Check if the endpoint has interrupt transfer type.
    #[inline]
    pub const fn xfer_int(&self) -> bool {
        (self.bm_attributes & bindings::USB_ENDPOINT_XFERTYPE_MASK as u8)
            == bindings::USB_ENDPOINT_XFER_INT as u8
    }

    /// Check if the endpoint has isochronous transfer type.
    #[inline]
    pub const fn xfer_isoc(&self) -> bool {
        (self.bm_attributes & bindings::USB_ENDPOINT_XFERTYPE_MASK as u8)
            == bindings::USB_ENDPOINT_XFER_ISOC as u8
    }

    /// Check if the endpoint is bulk IN.
    #[inline]
    pub const fn is_bulk_in(&self) -> bool {
        self.dir_in() && self.xfer_bulk()
    }

    /// Check if the endpoint is bulk OUT.
    #[inline]
    pub const fn is_bulk_out(&self) -> bool {
        self.dir_out() && self.xfer_bulk()
    }

    /// Check if the endpoint is interrupt IN.
    #[inline]
    pub const fn is_int_in(&self) -> bool {
        self.dir_in() && self.xfer_int()
    }

    /// Check if the endpoint is interrupt OUT.
    #[inline]
    pub const fn is_int_out(&self) -> bool {
        self.dir_out() && self.xfer_int()
    }

    /// Check if the endpoint is isochronous IN.
    #[inline]
    pub const fn is_isoc_in(&self) -> bool {
        self.dir_in() && self.xfer_isoc()
    }

    /// Check if the endpoint is isochronous OUT.
    #[inline]
    pub const fn is_isoc_out(&self) -> bool {
        self.dir_out() && self.xfer_isoc()
    }

    /// Get endpoint's max packet size.
    #[inline]
    pub const fn maxp(&self) -> u16 {
        u16::from_le(self.w_max_packet_size) & bindings::USB_ENDPOINT_MAXP_MASK as u16
    }

    /// Get endpoint's transactional opportunities.
    #[inline]
    pub const fn maxp_mult(&self) -> u16 {
        (u16::from_le(self.w_max_packet_size) & bindings::USB_EP_MAXP_MULT_MASK as u16)
            >> bindings::USB_EP_MAXP_MULT_SHIFT
    }
}

/// An USB device descriptor.
#[derive(Default, Copy, Clone, PartialEq)]
#[repr(C, packed)]
pub struct DeviceDescriptor {
    /// Size of descriptor.
    pub b_length: u8,
    /// Descriptor type.
    pub b_descriptor_type: u8,
    /// USB specification release number in Binary Coded Decimal.
    pub bcd_usb: bindings::__le16,
    /// Class of device.
    pub b_device_class: u8,
    /// Subclass of device.
    pub b_device_subclass: u8,
    /// Protocol of device.
    pub b_device_protocol: u8,
    /// Maximum packet size for Endpoint zero (only 8, 16, 32, or 64 are valid).
    pub b_max_packet_size0: u8,
    /// USB vendor ID for a device.
    pub id_vendor: bindings::__le16,
    /// Vendor-assigned product ID.
    pub id_product: bindings::__le16,
    /// Device release number in Binary Coded Decimal.
    pub bcd_device: bindings::__le16,
    /// Index of string descriptor describing manufacturer.
    pub i_manufacturer: u8,
    /// Index of string descriptor describing product.
    pub i_product: u8,
    /// Index of string descriptor describing the device's serial number.
    pub i_serial_number: u8,
    /// Number of possible configurations.
    pub b_num_configurations: u8,
}

/// An USB interface descriptor.
#[derive(Default, Copy, Clone, PartialEq)]
#[repr(C, packed)]
pub struct InterfaceDescriptor {
    /// Size of descriptor.
    pub b_length: u8,
    /// Descriptor type.
    pub b_descriptor_type: u8,
    /// Number of interface.
    pub b_interface_number: u8,
    /// Alternate setting.
    pub b_alternate_setting: u8,
    /// Number of endpoints.
    pub b_num_endpoints: u8,
    /// Class of interface.
    pub b_interface_class: u8,
    /// Subclass of interface.
    pub b_interface_subclass: u8,
    /// Protocol of interface.
    pub b_interface_protocol: u8,
    /// Index of string descriptor.
    pub i_interface: u8,
}

/// An USB host-side interface wrapper.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HostInterface {
    /// Interface descriptor of the host interface.
    pub desc: InterfaceDescriptor,
    extralen: i32,
    extra: *mut u8,
    endpoint: *mut bindings::usb_host_endpoint,
    string: *mut i8,
}

impl<'a> HostInterface {
    /// Search the alternate setting's endpoint descriptors for the first bulk-in, bulk-out,
    /// interrupt-in and interrupt-out endpoints and return them in the provided references (unless
    /// they are [`None`]).
    ///
    /// If a requested endpoint is not found, the corresponding reference is set to `None`.
    ///
    /// # Errors
    ///
    /// Returns [`ENXIO`] if none of the requested descriptors were found.
    #[inline]
    pub fn find_common_endpoints(
        &'a self,
        bulk_in: Option<&mut Option<&'a EndpointDescriptor>>,
        bulk_out: Option<&mut Option<&'a EndpointDescriptor>>,
        int_in: Option<&mut Option<&'a EndpointDescriptor>>,
        int_out: Option<&mut Option<&'a EndpointDescriptor>>,
    ) -> Result {
        // SAFETY: FFI call.
        to_result(unsafe {
            bindings::usb_find_common_endpoints(
                self as *const Self as *mut _,
                bulk_in
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                bulk_out
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                int_in
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                int_out
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
            )
        })
    }

    /// Search the alternate setting's endpoint descriptors for the last bulk-in, bulk-out,
    /// interrupt-in and interrupt-out endpoints and return them in the provided references (unless
    /// they are [`None`]).
    ///
    /// If a requested endpoint is not found, the corresponding reference is set to `None`.
    ///
    /// # Errors
    ///
    /// Returns [`ENXIO`] if none of the requested descriptors were found.
    #[inline]
    pub fn find_common_endpoints_reverse(
        &'a self,
        bulk_in: Option<&mut Option<&'a EndpointDescriptor>>,
        bulk_out: Option<&mut Option<&'a EndpointDescriptor>>,
        int_in: Option<&mut Option<&'a EndpointDescriptor>>,
        int_out: Option<&mut Option<&'a EndpointDescriptor>>,
    ) -> Result {
        // SAFETY: FFI call.
        to_result(unsafe {
            bindings::usb_find_common_endpoints_reverse(
                self as *const Self as *mut _,
                bulk_in
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                bulk_out
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                int_in
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
                int_out
                    .map_or(core::ptr::null_mut(), |r| {
                        r as *mut Option<&'a EndpointDescriptor>
                    })
                    .cast(),
            )
        })
    }

    /// Finds the first bulk-in endpoint.
    ///
    /// See [`find_common_endpoints`].
    #[inline]
    pub fn find_bulk_in_endpoint(&'a self, bulk_in: &mut Option<&'a EndpointDescriptor>) -> Result {
        self.find_common_endpoints(Some(bulk_in), None, None, None)
    }

    /// Finds the first bulk-out endpoint.
    ///
    /// See [`find_common_endpoints`].
    #[inline]
    pub fn find_bulk_out_endpoint(
        &'a self,
        bulk_out: &mut Option<&'a EndpointDescriptor>,
    ) -> Result {
        self.find_common_endpoints(None, Some(bulk_out), None, None)
    }

    /// Finds the first interrupt-in endpoint.
    ///
    /// See [`find_common_endpoints`].
    #[inline]
    pub fn find_int_in_endpoint(&'a self, int_in: &mut Option<&'a EndpointDescriptor>) -> Result {
        self.find_common_endpoints(None, None, Some(int_in), None)
    }

    /// Finds the first interrupt-out endpoint.
    ///
    /// See [`find_common_endpoints`].
    #[inline]
    pub fn find_int_out_endpoint(&'a self, int_out: &mut Option<&'a EndpointDescriptor>) -> Result {
        self.find_common_endpoints(None, None, None, Some(int_out))
    }

    /// Finds the last bulk-in endpoint.
    ///
    /// See [`find_common_endpoints_reverse`].
    #[inline]
    pub fn find_last_bulk_in_endpoint(
        &'a self,
        bulk_in: &mut Option<&'a EndpointDescriptor>,
    ) -> Result {
        self.find_common_endpoints_reverse(Some(bulk_in), None, None, None)
    }

    /// Finds the last bulk-out endpoint.
    ///
    /// See [`find_common_endpoints_reverse`].
    #[inline]
    pub fn find_last_bulk_out_endpoint(
        &'a self,
        bulk_out: &mut Option<&'a EndpointDescriptor>,
    ) -> Result {
        self.find_common_endpoints_reverse(None, Some(bulk_out), None, None)
    }

    /// Finds the last interrupt-in endpoint.
    ///
    /// See [`find_common_endpoints_reverse`].
    #[inline]
    pub fn find_last_int_in_endpoint(
        &'a self,
        int_in: &mut Option<&'a EndpointDescriptor>,
    ) -> Result {
        self.find_common_endpoints_reverse(None, None, Some(int_in), None)
    }

    /// Finds the last interrupt-out endpoint.
    ///
    /// See [`find_common_endpoints_reverse`].
    #[inline]
    pub fn find_last_int_out_endpoint(
        &'a self,
        int_out: &mut Option<&'a EndpointDescriptor>,
    ) -> Result {
        self.find_common_endpoints_reverse(None, None, None, Some(int_out))
    }

    /// Provides an slice view to the endpoint descriptors.
    #[inline]
    pub fn endpoints(&'a self) -> &'a [EndpointDescriptor] {
        if self.endpoint.is_null() {
            &[]
        } else {
            // SAFETY: An slice is built out of a valid pointer and `EndpointDescriptor` is an
            // alias to `usb_endpoint_descriptor`.
            unsafe {
                core::slice::from_raw_parts(
                    (self.endpoint as *const EndpointDescriptor).cast(),
                    self.desc.b_num_endpoints as usize,
                )
            }
        }
    }

    /// Provides a mutable slice view to the endpoint descriptors.
    #[inline]
    pub fn endpoints_mut(&'a mut self) -> &'a mut [EndpointDescriptor] {
        if self.endpoint.is_null() {
            &mut []
        } else {
            // SAFETY: An slice is built out of a valid pointer and `EndpointDescriptor` is an
            // alias to `usb_endpoint_descriptor`.
            unsafe {
                core::slice::from_raw_parts_mut(
                    self.endpoint.cast(),
                    self.desc.b_num_endpoints as usize,
                )
            }
        }
    }
}
