// SPDX-License-Identifier: GPL-2.0

//! Real-Time Clock (RTC) subsystem abstractions.
//!
//! C header: [`include/linux/rtc.h`](srctree/include/linux/rtc.h)

use crate::{
    bindings,
    device,
    error::*,
    ffi::c_int,
    prelude::*,
};
use core::{marker::PhantomData, ptr::NonNull};

/// Convert binary-coded decimal (BCD) byte to binary value.
#[inline(always)]
pub const fn bcd_to_bin(val: u8) -> u8 {
    (val & 0x0f) + ((val >> 4) * 10)
}

/// Convert binary value (0..99) to binary-coded decimal (BCD) byte.
#[inline(always)]
pub const fn bin_to_bcd(val: u8) -> u8 {
    ((val / 10) << 4) | (val % 10)
}

/// Representation of standard RTC date and time.
///
/// Mirrors `struct rtc_time` in [`include/linux/rtc.h`](srctree/include/linux/rtc.h).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtcTime {
    /// Seconds in minute (0..59, up to 60 for leap seconds).
    pub tm_sec: u32,
    /// Minutes in hour (0..59).
    pub tm_min: u32,
    /// Hours in day (0..23).
    pub tm_hour: u32,
    /// Day of the month (1..31).
    pub tm_mday: u32,
    /// Month of the year (0..11, where 0 is January).
    pub tm_mon: u32,
    /// Years since 1900 (e.g. 2026 is 126).
    pub tm_year: i32,
    /// Day of the week (0..6, Sunday is 0).
    pub tm_wday: u32,
    /// Day in year (0..365).
    pub tm_yday: u32,
    /// Daylight savings flag.
    pub tm_isdst: i32,
}

impl From<bindings::rtc_time> for RtcTime {
    fn from(tm: bindings::rtc_time) -> Self {
        Self {
            tm_sec: tm.tm_sec as u32,
            tm_min: tm.tm_min as u32,
            tm_hour: tm.tm_hour as u32,
            tm_mday: tm.tm_mday as u32,
            tm_mon: tm.tm_mon as u32,
            tm_year: tm.tm_year,
            tm_wday: tm.tm_wday as u32,
            tm_yday: tm.tm_yday as u32,
            tm_isdst: tm.tm_isdst,
        }
    }
}

impl From<RtcTime> for bindings::rtc_time {
    fn from(tm: RtcTime) -> Self {
        Self {
            tm_sec: tm.tm_sec as c_int,
            tm_min: tm.tm_min as c_int,
            tm_hour: tm.tm_hour as c_int,
            tm_mday: tm.tm_mday as c_int,
            tm_mon: tm.tm_mon as c_int,
            tm_year: tm.tm_year as c_int,
            tm_wday: tm.tm_wday as c_int,
            tm_yday: tm.tm_yday as c_int,
            tm_isdst: tm.tm_isdst as c_int,
        }
    }
}

/// Trait implemented by RTC hardware drivers.
pub trait Operations: Send + Sync + Sized {
    /// Read current time from RTC hardware.
    fn read_time(&self) -> Result<RtcTime>;

    /// Set current time in RTC hardware.
    fn set_time(&self, tm: &RtcTime) -> Result;
}

/// Adapter holding the C callbacks for a driver implementing [`Operations`].
pub struct Adapter<T: Operations>(PhantomData<T>);

impl<T: Operations> Adapter<T> {
    unsafe extern "C" fn read_time_callback(
        dev: *mut bindings::device,
        tm: *mut bindings::rtc_time,
    ) -> c_int {
        // SAFETY: `dev` is valid and `dev_get_drvdata` retrieves our `T`.
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() || tm.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.read_time() {
            Ok(time) => {
                unsafe { *tm = time.into() };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn set_time_callback(
        dev: *mut bindings::device,
        tm: *mut bindings::rtc_time,
    ) -> c_int {
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() || tm.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        let rtc_tm: RtcTime = unsafe { (*tm).into() };
        match op.set_time(&rtc_tm) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    /// Const vtable for RTC callbacks.
    pub const OPS: bindings::rtc_class_ops = bindings::rtc_class_ops {
        ioctl: None,
        read_time: Some(Self::read_time_callback),
        set_time: Some(Self::set_time_callback),
        read_alarm: None,
        set_alarm: None,
        proc_: None,
        alarm_irq_enable: None,
        read_offset: None,
        set_offset: None,
        param_get: None,
        param_set: None,
    };
}

/// Handle to registered RTC device.
pub struct Registration {
    #[expect(dead_code)]
    rtc_dev: NonNull<bindings::rtc_device>,
}

impl Registration {
    /// Register a new RTC device associated with `parent_dev`.
    pub fn register<T: Operations>(
        parent_dev: &device::Device,
        name: &'static CStr,
        _drvdata: &T,
        ops: &'static bindings::rtc_class_ops,
    ) -> Result<Self> {
        let parent_ptr = parent_dev.as_raw();

        // SAFETY: We pass valid arguments to `devm_rtc_device_register`.
        let ret = unsafe {
            bindings::devm_rtc_device_register(
                parent_ptr,
                name.as_char_ptr(),
                ops,
                core::ptr::null_mut(),
            )
        };

        let dev_ptr = NonNull::new(ret).ok_or(ENOMEM)?;
        Ok(Self { rtc_dev: dev_ptr })
    }
}

// SAFETY: RTC registration can be transferred across threads.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

#[cfg(CONFIG_KUNIT)]
mod tests {
    use super::*;

    #[kunit_test]
    fn test_bcd_conversions() {
        assert_eq!(bcd_to_bin(0x26), 26);
        assert_eq!(bcd_to_bin(0x59), 59);
        assert_eq!(bcd_to_bin(0x00), 0);

        assert_eq!(bin_to_bcd(26), 0x26);
        assert_eq!(bin_to_bcd(59), 0x59);
        assert_eq!(bin_to_bcd(0), 0x00);
    }
}
