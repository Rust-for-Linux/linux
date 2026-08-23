// SPDX-License-Identifier: GPL-2.0

//! NXP PCF8563 and compatible Real-Time Clock (RTC) driver in Rust.
//!
//! C version: `drivers/rtc/rtc-pcf8563.c`
//!
//! Handles PCF8563, Epson RTC8564, and NXP PCA8565 I2C RTC chips.

use kernel::{
    device,
    error::*,
    i2c,
    of,
    prelude::*,
    rtc::{self, bcd_to_bin, bin_to_bcd, Operations, RtcTime},
};

// PCF8563 Register Map.
const PCF8563_REG_ST1: u8 = 0x00;
#[expect(dead_code)]
const PCF8563_REG_ST2: u8 = 0x01;
const PCF8563_REG_SC: u8 = 0x02; // Seconds
const PCF8563_REG_MN: u8 = 0x03; // Minutes
const PCF8563_REG_HR: u8 = 0x04; // Hours
const PCF8563_REG_DM: u8 = 0x05; // Day of month
const PCF8563_REG_DW: u8 = 0x06; // Day of week
const PCF8563_REG_MO: u8 = 0x07; // Month
const PCF8563_REG_YR: u8 = 0x08; // Year

// Flags
const PCF8563_SC_LV: u8 = 0x80; // Low voltage / data invalid flag
#[expect(dead_code)]
const PCF8563_MO_C: u8 = 0x80; // Century bit

/// Supported chip variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    /// NXP PCF8563.
    Pcf8563,
    /// Epson RTC-8564.
    Rtc8564,
    /// NXP PCA8565.
    Pca8565,
}

// I2C Device ID table.
kernel::i2c_device_table!(
    I2C_TABLE,
    MODULE_I2C_TABLE,
    ChipKind,
    [
        (i2c::DeviceId::new(c"pcf8563"), ChipKind::Pcf8563),
        (i2c::DeviceId::new(c"rtc8564"), ChipKind::Rtc8564),
        (i2c::DeviceId::new(c"pca8565"), ChipKind::Pca8565),
    ]
);

// Device Tree (OpenFirmware) match table.
kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    ChipKind,
    [
        (of::DeviceId::new(c"nxp,pcf8563"), ChipKind::Pcf8563),
        (of::DeviceId::new(c"epson,rtc8564"), ChipKind::Rtc8564),
        (of::DeviceId::new(c"nxp,pca8565"), ChipKind::Pca8565),
    ]
);

/// Driver private data per probed instance.
pub struct Pcf8563Data {
    #[expect(dead_code)]
    kind: ChipKind,
    raw_client: *mut kernel::bindings::i2c_client,
    hwmon_reg: Option<rtc::Registration>,
}

// SAFETY: Data can be shared across threads.
unsafe impl Send for Pcf8563Data {}
unsafe impl Sync for Pcf8563Data {}

impl Pcf8563Data {
    fn read_reg(&self, reg: u8) -> Result<u8> {
        let val = unsafe { kernel::bindings::i2c_smbus_read_byte_data(self.raw_client, reg) };
        if val < 0 {
            Err(Error::from_errno(val))
        } else {
            Ok(val as u8)
        }
    }

    fn write_reg(&self, reg: u8, val: u8) -> Result {
        to_result(unsafe {
            kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, reg, val)
        })
    }
}

impl Operations for Pcf8563Data {
    fn read_time(&self) -> Result<RtcTime> {
        let sec_raw = self.read_reg(PCF8563_REG_SC)?;
        if sec_raw & PCF8563_SC_LV != 0 {
            pr_warn!("PCF8563: low voltage detected, RTC time is invalid\n");
            return Err(EINVAL);
        }

        let min_raw = self.read_reg(PCF8563_REG_MN)?;
        let hr_raw = self.read_reg(PCF8563_REG_HR)?;
        let mday_raw = self.read_reg(PCF8563_REG_DM)?;
        let wday_raw = self.read_reg(PCF8563_REG_DW)?;
        let mon_raw = self.read_reg(PCF8563_REG_MO)?;
        let yr_raw = self.read_reg(PCF8563_REG_YR)?;

        let tm_sec = bcd_to_bin(sec_raw & 0x7f) as u32;
        let tm_min = bcd_to_bin(min_raw & 0x7f) as u32;
        let tm_hour = bcd_to_bin(hr_raw & 0x3f) as u32;
        let tm_mday = bcd_to_bin(mday_raw & 0x3f) as u32;
        let tm_wday = (wday_raw & 0x07) as u32;
        let tm_mon = bcd_to_bin(mon_raw & 0x1f).saturating_sub(1) as u32;
        let tm_year = (bcd_to_bin(yr_raw) as i32) + 100;

        Ok(RtcTime {
            tm_sec,
            tm_min,
            tm_hour,
            tm_mday,
            tm_mon,
            tm_year,
            tm_wday,
            tm_yday: 0,
            tm_isdst: 0,
        })
    }

    fn set_time(&self, tm: &RtcTime) -> Result {
        let sec_bcd = bin_to_bcd(tm.tm_sec as u8);
        let min_bcd = bin_to_bcd(tm.tm_min as u8);
        let hr_bcd = bin_to_bcd(tm.tm_hour as u8);
        let mday_bcd = bin_to_bcd(tm.tm_mday as u8);
        let mon_bcd = bin_to_bcd((tm.tm_mon + 1) as u8);
        let yr_val = if tm.tm_year >= 100 {
            tm.tm_year - 100
        } else {
            tm.tm_year
        };
        let yr_bcd = bin_to_bcd(yr_val as u8);
        let wday_bcd = (tm.tm_wday & 0x07) as u8;

        self.write_reg(PCF8563_REG_SC, sec_bcd)?;
        self.write_reg(PCF8563_REG_MN, min_bcd)?;
        self.write_reg(PCF8563_REG_HR, hr_bcd)?;
        self.write_reg(PCF8563_REG_DM, mday_bcd)?;
        self.write_reg(PCF8563_REG_DW, wday_bcd)?;
        self.write_reg(PCF8563_REG_MO, mon_bcd)?;
        self.write_reg(PCF8563_REG_YR, yr_bcd)?;

        Ok(())
    }
}

/// The PCF8563 I2C driver structure.
struct Pcf8563Driver;

impl i2c::Driver for Pcf8563Driver {
    type IdInfo = ChipKind;
    type Data<'bound> = Pcf8563Data;

    const I2C_ID_TABLE: Option<i2c::IdTable<Self::IdInfo>> = Some(&I2C_TABLE);
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        dev: &'bound i2c::I2cClient<device::Core<'_>>,
        id_info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        let kind = id_info.copied().unwrap_or(ChipKind::Pcf8563);
        let raw_client = dev.as_raw();

        pr_info!("PCF8563 Rust driver probing device at I2C adapter\n");

        // Verify communication by reading status register 1.
        let status = unsafe { kernel::bindings::i2c_smbus_read_byte_data(raw_client, PCF8563_REG_ST1) };
        if status < 0 {
            pr_err!("PCF8563 Rust: failed to read status register: {}\n", status);
            return Err(Error::from_errno(status));
        }

        pr_info!("PCF8563 Rust: device detected (st1=0x{:02x})\n", status);

        let mut data = Pcf8563Data {
            kind,
            raw_client,
            hwmon_reg: None,
        };

        // Register RTC device with kernel RTC class.
        match rtc::Registration::register(
            dev.as_ref(),
            c"rtc_pcf8563_rust",
            &data,
            &rtc::Adapter::<Pcf8563Data>::OPS,
        ) {
            Ok(reg) => {
                pr_info!("PCF8563 Rust: registered as RTC device\n");
                data.hwmon_reg = Some(reg);
            }
            Err(e) => {
                pr_warn!("PCF8563 Rust: failed to register RTC device: {:?}\n", e);
            }
        }

        Ok(data)
    }
}

kernel::module_i2c_driver! {
    type: Pcf8563Driver,
    name: "rtc_pcf8563_rust",
    authors: ["Rust for Linux Developers"],
    description: "Rust PCF8563 Real-Time Clock Driver",
    license: "GPL",
}
