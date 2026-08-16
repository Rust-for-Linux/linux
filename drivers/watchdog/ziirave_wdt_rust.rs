// SPDX-License-Identifier: GPL-2.0-or-later

//! Zodiac Inflight Innovations RAVE Watchdog Processor driver in Rust.
//!
//! C version: `drivers/watchdog/ziirave_wdt.c`
//!
//! Handles the I2C-connected RAVE watchdog processor used in aerospace and embedded Linux systems.

use kernel::{
    bindings,
    device,
    error::*,
    i2c,
    of,
    prelude::*,
    watchdog::{self, Operations, WatchdogFlags},
};

// ZII RAVE Watchdog Registers.
const ZIIRAVE_WDT_STATE: u8 = 0x06;
const ZIIRAVE_WDT_TIMEOUT: u8 = 0x07;
#[expect(dead_code)]
const ZIIRAVE_WDT_TIME_LEFT: u8 = 0x08;
const ZIIRAVE_WDT_PING: u8 = 0x09;

// State Register commands.
const ZIIRAVE_STATE_OFF: u8 = 0x01;
const ZIIRAVE_STATE_ON: u8 = 0x02;
const ZIIRAVE_PING_VALUE: u8 = 0x00;

// Timeout limits in seconds.
const ZIIRAVE_TIMEOUT_MIN: u32 = 3;
const ZIIRAVE_TIMEOUT_MAX: u32 = 255;
const ZIIRAVE_TIMEOUT_DEFAULT: u32 = 30;

/// Watchdog Device info metadata.
static ZIIRAVE_WDT_INFO: bindings::watchdog_info = bindings::watchdog_info {
    options: WatchdogFlags::SETTIMEOUT | WatchdogFlags::KEEPALIVEPING | WatchdogFlags::MAGICCLOSE,
    firmware_version: 1,
    identity: *b"ZII RAVE Watchdog (Rust)\0\0\0\0\0\0\0\0",
};

// I2C Device ID table.
kernel::i2c_device_table!(
    I2C_TABLE,
    MODULE_I2C_TABLE,
    (),
    [
        (i2c::DeviceId::new(c"ziirave-wdt"), ()),
    ]
);

// Device Tree (OpenFirmware) match table.
kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    (),
    [
        (of::DeviceId::new(c"zii,rave-wdt"), ()),
    ]
);

/// Driver private data per probed instance.
pub struct ZiiraveWdtData {
    raw_client: *mut kernel::bindings::i2c_client,
    wdt_reg: Option<watchdog::Registration>,
}

// SAFETY: Private data is thread-safe.
unsafe impl Send for ZiiraveWdtData {}
unsafe impl Sync for ZiiraveWdtData {}

impl ZiiraveWdtData {
    fn write_byte(&self, reg: u8, val: u8) -> Result {
        to_result(unsafe {
            kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, reg, val)
        })
    }
}

impl Operations for ZiiraveWdtData {
    fn start(&self) -> Result {
        pr_info!("ZIIRAVE Watchdog Rust: starting timer\n");
        self.write_byte(ZIIRAVE_WDT_STATE, ZIIRAVE_STATE_ON)
    }

    fn stop(&self) -> Result {
        pr_info!("ZIIRAVE Watchdog Rust: stopping timer\n");
        self.write_byte(ZIIRAVE_WDT_STATE, ZIIRAVE_STATE_OFF)
    }

    fn ping(&self) -> Result {
        self.write_byte(ZIIRAVE_WDT_PING, ZIIRAVE_PING_VALUE)
    }

    fn set_timeout(&self, timeout: u32) -> Result {
        let clamped = timeout.clamp(ZIIRAVE_TIMEOUT_MIN, ZIIRAVE_TIMEOUT_MAX);
        pr_info!("ZIIRAVE Watchdog Rust: setting timeout to {}s\n", clamped);
        self.write_byte(ZIIRAVE_WDT_TIMEOUT, clamped as u8)
    }
}

/// The ZII RAVE Watchdog I2C driver structure.
struct ZiiraveWdtDriver;

impl i2c::Driver for ZiiraveWdtDriver {
    type IdInfo = ();
    type Data<'bound> = ZiiraveWdtData;

    const I2C_ID_TABLE: Option<i2c::IdTable<Self::IdInfo>> = Some(&I2C_TABLE);
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        dev: &'bound i2c::I2cClient<device::Core<'_>>,
        _id_info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        let raw_client = dev.as_raw();

        pr_info!("ZIIRAVE Watchdog Rust driver probing device at I2C adapter\n");

        let mut data = ZiiraveWdtData {
            raw_client,
            wdt_reg: None,
        };

        // Initialize default timeout.
        let _ = data.set_timeout(ZIIRAVE_TIMEOUT_DEFAULT);

        // Register watchdog device with kernel watchdog core.
        match watchdog::Registration::register(
            dev.as_ref(),
            &ZIIRAVE_WDT_INFO,
            &data,
            ZIIRAVE_TIMEOUT_MIN,
            ZIIRAVE_TIMEOUT_MAX,
            ZIIRAVE_TIMEOUT_DEFAULT,
        ) {
            Ok(reg) => {
                pr_info!("ZIIRAVE Watchdog Rust: registered watchdog device\n");
                data.wdt_reg = Some(reg);
            }
            Err(e) => {
                pr_warn!("ZIIRAVE Watchdog Rust: failed to register: {:?}\n", e);
            }
        }

        Ok(data)
    }
}

kernel::module_i2c_driver! {
    type: ZiiraveWdtDriver,
    name: "ziirave_wdt_rust",
    authors: ["Rust for Linux Developers"],
    description: "Rust Zodiac RAVE I2C Watchdog Driver",
    license: "GPL",
}
