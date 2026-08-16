// SPDX-License-Identifier: GPL-2.0

//! Hardware Monitoring (HWMON) subsystem abstractions.
//!
//! C header: [`include/linux/hwmon.h`](srctree/include/linux/hwmon.h)

use crate::{
    bindings,
    device,
    error::*,
    ffi::{c_int, c_long, c_void},
    prelude::*,
};
use core::{marker::PhantomData, ptr::NonNull};

/// Sensor types recognized by the HWMON subsystem.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SensorType {
    /// Chip-level attributes.
    Chip = bindings::hwmon_sensor_types_hwmon_chip,
    /// Temperature sensor.
    Temp = bindings::hwmon_sensor_types_hwmon_temp,
    /// Voltage sensor.
    In = bindings::hwmon_sensor_types_hwmon_in,
    /// Current sensor.
    Curr = bindings::hwmon_sensor_types_hwmon_curr,
    /// Power sensor.
    Power = bindings::hwmon_sensor_types_hwmon_power,
    /// Fan speed sensor.
    Fan = bindings::hwmon_sensor_types_hwmon_fan,
    /// PWM output control.
    Pwm = bindings::hwmon_sensor_types_hwmon_pwm,
}

impl SensorType {
    fn from_raw(raw: bindings::hwmon_sensor_types) -> Option<Self> {
        match raw {
            bindings::hwmon_sensor_types_hwmon_chip => Some(Self::Chip),
            bindings::hwmon_sensor_types_hwmon_temp => Some(Self::Temp),
            bindings::hwmon_sensor_types_hwmon_in => Some(Self::In),
            bindings::hwmon_sensor_types_hwmon_curr => Some(Self::Curr),
            bindings::hwmon_sensor_types_hwmon_power => Some(Self::Power),
            bindings::hwmon_sensor_types_hwmon_fan => Some(Self::Fan),
            bindings::hwmon_sensor_types_hwmon_pwm => Some(Self::Pwm),
            _ => None,
        }
    }
}

/// Temperature sensor attributes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TempAttribute {
    /// Current temperature input value in millicelsius.
    Input = bindings::hwmon_temp_attributes_hwmon_temp_input,
    /// Maximum temperature threshold in millicelsius.
    Max = bindings::hwmon_temp_attributes_hwmon_temp_max,
    /// Hysteresis temperature threshold for max in millicelsius.
    MaxHyst = bindings::hwmon_temp_attributes_hwmon_temp_max_hyst,
    /// Minimum temperature threshold in millicelsius.
    Min = bindings::hwmon_temp_attributes_hwmon_temp_min,
    /// Critical temperature threshold.
    Crit = bindings::hwmon_temp_attributes_hwmon_temp_crit,
    /// Alarm bit indicating temperature threshold breach.
    Alarm = bindings::hwmon_temp_attributes_hwmon_temp_alarm,
    /// Text label for temperature sensor.
    Label = bindings::hwmon_temp_attributes_hwmon_temp_label,
}

/// Safe wrapper around `bindings::hwmon_channel_info`.
pub struct ChannelInfo(bindings::hwmon_channel_info);

// SAFETY: ChannelInfo contains immutable descriptors safe to share across threads.
unsafe impl Send for ChannelInfo {}
unsafe impl Sync for ChannelInfo {}

impl ChannelInfo {
    /// Create a new static ChannelInfo.
    pub const fn new(sensor_type: bindings::hwmon_sensor_types, config: &'static [u32]) -> Self {
        Self(bindings::hwmon_channel_info {
            type_: sensor_type,
            config: config.as_ptr(),
        })
    }

    /// Return a raw pointer to the channel info struct.
    pub const fn as_ptr(&self) -> *const bindings::hwmon_channel_info {
        &self.0
    }
}

/// Safe wrapper around an array of channel info pointers.
pub struct ChannelInfoList<const N: usize>([*const bindings::hwmon_channel_info; N]);

// SAFETY: Pointer array is static and read-only.
unsafe impl<const N: usize> Send for ChannelInfoList<N> {}
unsafe impl<const N: usize> Sync for ChannelInfoList<N> {}

impl<const N: usize> ChannelInfoList<N> {
    /// Create a new channel info pointer list.
    pub const fn new(list: [*const bindings::hwmon_channel_info; N]) -> Self {
        Self(list)
    }

    /// Return a pointer to the head of the list.
    pub const fn as_ptr(&self) -> *const *const bindings::hwmon_channel_info {
        self.0.as_ptr()
    }
}

/// Safe wrapper around `bindings::hwmon_chip_info`.
pub struct ChipInfo(bindings::hwmon_chip_info);

// SAFETY: ChipInfo is static and thread-safe.
unsafe impl Send for ChipInfo {}
unsafe impl Sync for ChipInfo {}

impl ChipInfo {
    /// Create a new static ChipInfo.
    pub const fn new<const N: usize>(
        ops: &'static bindings::hwmon_ops,
        channel_list: &'static ChannelInfoList<N>,
    ) -> Self {
        Self(bindings::hwmon_chip_info {
            ops,
            info: channel_list.as_ptr(),
        })
    }

    /// Return the raw pointer to the chip info struct.
    pub fn as_raw(&self) -> *const bindings::hwmon_chip_info {
        &self.0
    }
}

/// Operations trait for HWMON drivers.
pub trait Operations: Send + Sync + Sized {
    /// Read a sensor attribute value.
    fn read(&self, type_: SensorType, attr: u32, channel: i32) -> Result<i64> {
        let _ = (type_, attr, channel);
        Err(EINVAL)
    }

    /// Write a sensor attribute value.
    fn write(&self, type_: SensorType, attr: u32, channel: i32, val: i64) -> Result {
        let _ = (type_, attr, channel, val);
        Err(EINVAL)
    }

    /// Check if an attribute is visible and return its file mode permissions (e.g. 0o444, 0o644).
    fn is_visible(&self, type_: SensorType, attr: u32, channel: i32) -> u16 {
        let _ = (type_, attr, channel);
        0
    }
}

/// Adapter holding the C callbacks for a driver implementing [`Operations`].
pub struct Adapter<T: Operations>(PhantomData<T>);

impl<T: Operations> Adapter<T> {
    unsafe extern "C" fn is_visible_callback(
        drvdata: *const c_void,
        type_: bindings::hwmon_sensor_types,
        attr: u32,
        channel: c_int,
    ) -> bindings::umode_t {
        if drvdata.is_null() {
            return 0;
        }
        // SAFETY: `drvdata` was passed during registration and points to `T`.
        let op = unsafe { &*drvdata.cast::<T>() };
        match SensorType::from_raw(type_) {
            Some(st) => op.is_visible(st, attr, channel),
            None => 0,
        }
    }

    unsafe extern "C" fn read_callback(
        dev: *mut bindings::device,
        type_: bindings::hwmon_sensor_types,
        attr: u32,
        channel: c_int,
        val: *mut c_long,
    ) -> c_int {
        // SAFETY: `dev` is valid and `dev_get_drvdata` retrieves our `T`.
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() || val.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        let st = match SensorType::from_raw(type_) {
            Some(s) => s,
            None => return -(bindings::EINVAL as c_int),
        };

        match op.read(st, attr, channel) {
            Ok(v) => {
                unsafe { *val = v as c_long };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn write_callback(
        dev: *mut bindings::device,
        type_: bindings::hwmon_sensor_types,
        attr: u32,
        channel: c_int,
        val: c_long,
    ) -> c_int {
        // SAFETY: `dev` is valid.
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        let st = match SensorType::from_raw(type_) {
            Some(s) => s,
            None => return -(bindings::EINVAL as c_int),
        };

        match op.write(st, attr, channel, val as i64) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    /// Const vtable for HWMON callbacks.
    pub const OPS: bindings::hwmon_ops = bindings::hwmon_ops {
        visible: 0,
        is_visible: Some(Self::is_visible_callback),
        read: Some(Self::read_callback),
        read_string: None,
        write: Some(Self::write_callback),
    };
}

/// Registration handle of a HWMON device. Unregisters device upon `Drop`.
pub struct Registration {
    #[expect(dead_code)]
    hwmon_dev: NonNull<bindings::device>,
}

impl Registration {
    /// Register a new HWMON device attached to `parent_dev`.
    pub fn register<T: Operations>(
        parent_dev: &device::Device,
        name: &'static CStr,
        drvdata: &T,
        chip_info: &'static ChipInfo,
    ) -> Result<Self> {
        let parent_ptr = parent_dev.as_raw();
        let data_ptr = (drvdata as *const T).cast::<c_void>() as *mut c_void;

        // SAFETY: We pass valid pointers to `devm_hwmon_device_register_with_info`.
        let ret = unsafe {
            bindings::devm_hwmon_device_register_with_info(
                parent_ptr,
                name.as_char_ptr(),
                data_ptr,
                chip_info.as_raw(),
                core::ptr::null_mut(),
            )
        };

        let dev_ptr = NonNull::new(ret).ok_or(ENOMEM)?;
        Ok(Self { hwmon_dev: dev_ptr })
    }
}

// SAFETY: HWMON registration can be moved between threads.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}
