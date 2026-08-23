// SPDX-License-Identifier: GPL-2.0

//! LM75 and compatible digital temperature sensor driver in Rust.
//!
//! C version: `drivers/hwmon/lm75.c`
//!
//! This driver handles LM75 and compatible I2C temperature sensors,
//! including TI TMP75, TMP102, Microchip MCP980x, Dallas DS75, etc.

use kernel::{
    bindings,
    device,
    error::*,
    hwmon::{self, Operations, SensorType, TempAttribute},
    i2c,
    of,
    prelude::*,
};

// LM75 Register addresses.
const LM75_REG_TEMP: u8 = 0x00;
const LM75_REG_CONF: u8 = 0x01;
const LM75_REG_HYST: u8 = 0x02;
const LM75_REG_MAX: u8 = 0x03;

// Configuration register bit flags.
const LM75_CONF_SHUTDOWN: u8 = 0x01;

// Temperature bounds in milli-degrees Celsius (-55°C to +125°C).
const LM75_TEMP_MIN: i32 = -55_000;
const LM75_TEMP_MAX: i32 = 125_000;

/// Chip variants supported by the driver.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    /// National Semiconductor LM75.
    Lm75,
    /// National Semiconductor / NXP LM75A with higher resolution.
    Lm75a,
    /// Texas Instruments TMP75.
    Tmp75,
    /// Texas Instruments TMP102.
    Tmp102,
    /// Dallas Semiconductor DS75.
    Ds75,
    /// Microchip MCP980x series.
    Mcp980x,
}

// I2C Device ID table.
kernel::i2c_device_table!(
    I2C_TABLE,
    MODULE_I2C_TABLE,
    ChipKind,
    [
        (i2c::DeviceId::new(c"lm75"), ChipKind::Lm75),
        (i2c::DeviceId::new(c"lm75a"), ChipKind::Lm75a),
        (i2c::DeviceId::new(c"tmp75"), ChipKind::Tmp75),
        (i2c::DeviceId::new(c"tmp102"), ChipKind::Tmp102),
        (i2c::DeviceId::new(c"ds75"), ChipKind::Ds75),
        (i2c::DeviceId::new(c"mcp980x"), ChipKind::Mcp980x),
    ]
);

// Device Tree (OpenFirmware) match table.
kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    ChipKind,
    [
        (of::DeviceId::new(c"national,lm75"), ChipKind::Lm75),
        (of::DeviceId::new(c"national,lm75a"), ChipKind::Lm75a),
        (of::DeviceId::new(c"ti,tmp75"), ChipKind::Tmp75),
        (of::DeviceId::new(c"ti,tmp102"), ChipKind::Tmp102),
        (of::DeviceId::new(c"dallas,ds75"), ChipKind::Ds75),
        (of::DeviceId::new(c"microchip,mcp980x"), ChipKind::Mcp980x),
    ]
);

/// Driver private data per probed device instance.
pub struct Lm75Data {
    /// The detected chip variant.
    #[expect(dead_code)]
    kind: ChipKind,
    raw_client: *mut kernel::bindings::i2c_client,
    hwmon_reg: Option<hwmon::Registration>,
}

// SAFETY: Lm75Data can be safely sent and shared across threads.
unsafe impl Send for Lm75Data {}
unsafe impl Sync for Lm75Data {}

impl Lm75Data {
    /// Read 16-bit word from I2C device with byte-swapping (LM75 sends MSB first).
    fn read_temp_reg(&self, reg: u8) -> Result<u16> {
        let val = unsafe { kernel::bindings::i2c_smbus_read_word_data(self.raw_client, reg) };
        if val < 0 {
            Err(Error::from_errno(val))
        } else {
            Ok((val as u16).swap_bytes())
        }
    }

    /// Write 16-bit word to I2C device with byte-swapping.
    fn write_temp_reg(&self, reg: u8, val: u16) -> Result {
        to_result(unsafe {
            kernel::bindings::i2c_smbus_write_word_data(self.raw_client, reg, val.swap_bytes())
        })
    }

    /// Read 8-bit configuration register.
    fn read_config(&self) -> Result<u8> {
        let val = unsafe { kernel::bindings::i2c_smbus_read_byte_data(self.raw_client, LM75_REG_CONF) };
        if val < 0 {
            Err(Error::from_errno(val))
        } else {
            Ok(val as u8)
        }
    }

    /// Write 8-bit configuration register.
    fn write_config(&self, val: u8) -> Result {
        to_result(unsafe {
            kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, LM75_REG_CONF, val)
        })
    }

    /// Read current temperature in millicelsius.
    pub fn read_temperature(&self) -> Result<i32> {
        let raw = self.read_temp_reg(LM75_REG_TEMP)?;
        Ok(temp_from_reg(raw))
    }

    /// Read over-temperature shutdown threshold (T_max) in millicelsius.
    pub fn read_max_temperature(&self) -> Result<i32> {
        let raw = self.read_temp_reg(LM75_REG_MAX)?;
        Ok(temp_from_reg(raw))
    }

    /// Set over-temperature shutdown threshold (T_max) in millicelsius.
    pub fn set_max_temperature(&self, temp_millicelsius: i32) -> Result {
        let reg = temp_to_reg(temp_millicelsius);
        self.write_temp_reg(LM75_REG_MAX, reg)
    }

    /// Read hysteresis temperature threshold (T_hyst) in millicelsius.
    pub fn read_hyst_temperature(&self) -> Result<i32> {
        let raw = self.read_temp_reg(LM75_REG_HYST)?;
        Ok(temp_from_reg(raw))
    }

    /// Set hysteresis temperature threshold (T_hyst) in millicelsius.
    pub fn set_hyst_temperature(&self, temp_millicelsius: i32) -> Result {
        let reg = temp_to_reg(temp_millicelsius);
        self.write_temp_reg(LM75_REG_HYST, reg)
    }
}

impl Operations for Lm75Data {
    fn is_visible(&self, type_: SensorType, attr: u32, _channel: i32) -> u16 {
        if type_ == SensorType::Temp {
            match attr {
                x if x == TempAttribute::Input as u32 => 0o444,
                x if x == TempAttribute::Max as u32 => 0o644,
                x if x == TempAttribute::MaxHyst as u32 => 0o644,
                _ => 0,
            }
        } else {
            0
        }
    }

    fn read(&self, type_: SensorType, attr: u32, _channel: i32) -> Result<i64> {
        if type_ != SensorType::Temp {
            return Err(EINVAL);
        }
        match attr {
            x if x == TempAttribute::Input as u32 => self.read_temperature().map(|v| v as i64),
            x if x == TempAttribute::Max as u32 => self.read_max_temperature().map(|v| v as i64),
            x if x == TempAttribute::MaxHyst as u32 => self.read_hyst_temperature().map(|v| v as i64),
            _ => Err(EINVAL),
        }
    }

    fn write(&self, type_: SensorType, attr: u32, _channel: i32, val: i64) -> Result {
        if type_ != SensorType::Temp {
            return Err(EINVAL);
        }
        match attr {
            x if x == TempAttribute::Max as u32 => self.set_max_temperature(val as i32),
            x if x == TempAttribute::MaxHyst as u32 => self.set_hyst_temperature(val as i32),
            _ => Err(EINVAL),
        }
    }
}

// Temperature channel configuration bitmask.
const LM75_TEMP_CONFIG: [u32; 2] = [
    (1 << bindings::hwmon_temp_attributes_hwmon_temp_input)
        | (1 << bindings::hwmon_temp_attributes_hwmon_temp_max)
        | (1 << bindings::hwmon_temp_attributes_hwmon_temp_max_hyst),
    0,
];

static LM75_TEMP_CHANNEL_INFO: hwmon::ChannelInfo =
    hwmon::ChannelInfo::new(bindings::hwmon_sensor_types_hwmon_temp, &LM75_TEMP_CONFIG);

static LM75_CHANNEL_INFO_LIST: hwmon::ChannelInfoList<2> =
    hwmon::ChannelInfoList::new([LM75_TEMP_CHANNEL_INFO.as_ptr(), core::ptr::null()]);

static LM75_CHIP_INFO: hwmon::ChipInfo =
    hwmon::ChipInfo::new(&hwmon::Adapter::<Lm75Data>::OPS, &LM75_CHANNEL_INFO_LIST);

/// Convert temperature from millicelsius to 16-bit LM75 register format.
///
/// Format: 9-bit two's complement, 0.5°C LSB, left-aligned in 16-bit word.
pub fn temp_to_reg(temp_millicelsius: i32) -> u16 {
    let clamped = temp_millicelsius.clamp(LM75_TEMP_MIN, LM75_TEMP_MAX);
    let rounded = if clamped < 0 {
        clamped - 250
    } else {
        clamped + 250
    };
    ((rounded / 500) as u16) << 7
}

/// Convert 16-bit LM75 register value to millicelsius.
///
/// Arithmetic division is used to preserve sign during right shift.
pub fn temp_from_reg(reg: u16) -> i32 {
    ((reg as i16) / 128) as i32 * 500
}

/// The LM75 I2C driver structure.
struct Lm75Driver;

impl i2c::Driver for Lm75Driver {
    type IdInfo = ChipKind;
    type Data<'bound> = Lm75Data;

    const I2C_ID_TABLE: Option<i2c::IdTable<Self::IdInfo>> = Some(&I2C_TABLE);
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        dev: &'bound i2c::I2cClient<device::Core<'_>>,
        id_info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        let kind = id_info.copied().unwrap_or(ChipKind::Lm75);
        let raw_client = dev.as_raw();

        pr_info!("LM75 Rust driver probing device at I2C adapter\n");

        // Verify device communication by reading config register.
        let status = unsafe { kernel::bindings::i2c_smbus_read_byte_data(raw_client, LM75_REG_CONF) };
        if status < 0 {
            pr_err!("LM75 Rust: failed to read config register: {}\n", status);
            return Err(Error::from_errno(status));
        }

        pr_info!("LM75 Rust: device successfully detected (conf=0x{:02x})\n", status);

        // Wake up chip from shutdown mode if set.
        let conf = status as u8;
        if conf & LM75_CONF_SHUTDOWN != 0 {
            let new_conf = conf & !LM75_CONF_SHUTDOWN;
            let _ = unsafe { kernel::bindings::i2c_smbus_write_byte_data(raw_client, LM75_REG_CONF, new_conf) };
        }

        // Read initial temperature reading to verify sensor data path.
        let temp_raw = unsafe { kernel::bindings::i2c_smbus_read_word_data(raw_client, LM75_REG_TEMP) };
        if temp_raw >= 0 {
            let temp_swapped = (temp_raw as u16).swap_bytes();
            let temp_mc = temp_from_reg(temp_swapped);
            pr_info!("LM75 Rust: Initial temperature: {}.{}°C\n", temp_mc / 1000, (temp_mc.abs() % 1000) / 100);
        }

        let mut data = Lm75Data {
            kind,
            raw_client,
            hwmon_reg: None,
        };

        // Register with the kernel HWMON subsystem.
        match hwmon::Registration::register(dev.as_ref(), c"lm75", &data, &LM75_CHIP_INFO) {
            Ok(reg) => {
                pr_info!("LM75 Rust: registered with HWMON subsystem\n");
                data.hwmon_reg = Some(reg);
            }
            Err(e) => {
                pr_warn!("LM75 Rust: failed to register with HWMON: {:?}\n", e);
            }
        }

        Ok(data)
    }

    fn shutdown<'bound>(_dev: &'bound i2c::I2cClient<device::Core<'_>>, this: Pin<&Self::Data<'bound>>) {
        pr_info!("LM75 Rust: putting device into low-power shutdown mode\n");
        if let Ok(conf) = this.read_config() {
            let _ = this.write_config(conf | LM75_CONF_SHUTDOWN);
        }
    }
}

kernel::module_i2c_driver! {
    type: Lm75Driver,
    name: "lm75_rust",
    authors: ["Rust for Linux Developers"],
    description: "Rust LM75 I2C Temperature Sensor Driver",
    license: "GPL",
}

#[cfg(CONFIG_KUNIT)]
mod tests {
    use super::*;

    #[kunit_test]
    fn test_temp_conversion_positive() {
        // 25.0°C = 25000 millicelsius -> 25000 / 500 = 50 -> 50 << 7 = 0x1900
        let reg = temp_to_reg(25_000);
        let back = temp_from_reg(reg);
        assert_eq!(back, 25_000);
    }

    #[kunit_test]
    fn test_temp_conversion_zero() {
        let reg = temp_to_reg(0);
        let back = temp_from_reg(reg);
        assert_eq!(back, 0);
    }

    #[kunit_test]
    fn test_temp_conversion_negative() {
        // -25.0°C = -25000 millicelsius
        let reg = temp_to_reg(-25_000);
        let back = temp_from_reg(reg);
        assert_eq!(back, -25_000);
    }

    #[kunit_test]
    fn test_temp_conversion_clamping() {
        // Value above max (150°C) should be clamped to 125°C
        let reg_max = temp_to_reg(150_000);
        assert_eq!(temp_from_reg(reg_max), 125_000);

        // Value below min (-80°C) should be clamped to -55°C
        let reg_min = temp_to_reg(-80_000);
        assert_eq!(temp_from_reg(reg_min), -55_000);
    }
}
