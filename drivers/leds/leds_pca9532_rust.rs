// SPDX-License-Identifier: GPL-2.0

//! NXP PCA9532 / PCA9533 / PCA9530 / PCA9531 I2C LED Dimmer Driver in Rust.
//!
//! C version: `drivers/leds/leds-pca9532.c`
//!
//! Handles NXP PCA953x series 2/4/8/16-bit I2C LED dimmers with hardware PWM blinking.

use kernel::{
    device,
    error::*,
    i2c,
    leds::{self, Brightness, Operations},
    of,
    prelude::*,
};

// PCA953x Register Map.
const PCA9532_REG_PSC0: u8 = 0x01; // Frequency prescaler 0
const PCA9532_REG_PWM0: u8 = 0x02; // PWM duty cycle 0
const PCA9532_REG_PSC1: u8 = 0x03; // Frequency prescaler 1
const PCA9532_REG_PWM1: u8 = 0x04; // PWM duty cycle 1
const PCA9532_REG_LS0: u8 = 0x05;  // LED0..LED3 selector
#[expect(dead_code)]
const PCA9532_REG_LS1: u8 = 0x06;  // LED4..LED7 selector

// LED State Selector bits (2 bits per LED).
const PCA9532_LED_OFF: u8 = 0b00;
const PCA9532_LED_ON: u8 = 0b01;
const PCA9532_LED_PWM0: u8 = 0b10;
const PCA9532_LED_PWM1: u8 = 0b11;

/// Supported chip variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    /// PCA9530 (2 LEDs).
    Pca9530,
    /// PCA9531 (8 LEDs).
    Pca9531,
    /// PCA9532 (16 LEDs).
    Pca9532,
    /// PCA9533 (4 LEDs).
    Pca9533,
}

// I2C Device ID table.
kernel::i2c_device_table!(
    I2C_TABLE,
    MODULE_I2C_TABLE,
    ChipKind,
    [
        (i2c::DeviceId::new(c"pca9530"), ChipKind::Pca9530),
        (i2c::DeviceId::new(c"pca9531"), ChipKind::Pca9531),
        (i2c::DeviceId::new(c"pca9532"), ChipKind::Pca9532),
        (i2c::DeviceId::new(c"pca9533"), ChipKind::Pca9533),
    ]
);

// Device Tree (OpenFirmware) match table.
kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    ChipKind,
    [
        (of::DeviceId::new(c"nxp,pca9530"), ChipKind::Pca9530),
        (of::DeviceId::new(c"nxp,pca9531"), ChipKind::Pca9531),
        (of::DeviceId::new(c"nxp,pca9532"), ChipKind::Pca9532),
        (of::DeviceId::new(c"nxp,pca9533"), ChipKind::Pca9533),
    ]
);

/// Driver private data per probed instance.
pub struct Pca9532Data {
    #[expect(dead_code)]
    kind: ChipKind,
    raw_client: *mut kernel::bindings::i2c_client,
    led_reg: Option<leds::Registration>,
}

// SAFETY: Data can be shared across threads safely.
unsafe impl Send for Pca9532Data {}
unsafe impl Sync for Pca9532Data {}

impl Pca9532Data {
    fn write_reg(&self, reg: u8, val: u8) -> Result {
        to_result(unsafe {
            kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, reg, val)
        })
    }

    fn read_reg(&self, reg: u8) -> Result<u8> {
        let val = unsafe { kernel::bindings::i2c_smbus_read_byte_data(self.raw_client, reg) };
        if val < 0 {
            Err(Error::from_errno(val))
        } else {
            Ok(val as u8)
        }
    }
}

impl Operations for Pca9532Data {
    fn brightness_set(&self, brightness: u32) -> Result {
        let (state, pwm_val) = match brightness {
            Brightness::OFF => (PCA9532_LED_OFF, 0),
            Brightness::FULL => (PCA9532_LED_ON, 255),
            val => (PCA9532_LED_PWM0, val as u8),
        };

        if state == PCA9532_LED_PWM0 {
            // Set PWM0 duty cycle.
            self.write_reg(PCA9532_REG_PWM0, pwm_val)?;
            // Set default blink rate (152 Hz).
            self.write_reg(PCA9532_REG_PSC0, 0)?;
        }

        // Update LED 0 selector in LS0 register (bits 1:0).
        let current_ls0 = self.read_reg(PCA9532_REG_LS0).unwrap_or(0);
        let new_ls0 = (current_ls0 & !0x03) | (state & 0x03);
        self.write_reg(PCA9532_REG_LS0, new_ls0)?;

        Ok(())
    }

    fn blink_set(&self, delay_on: &mut usize, delay_off: &mut usize) -> Result {
        if *delay_on == 0 && *delay_off == 0 {
            *delay_on = 500;
            *delay_off = 500;
        }

        let total_ms = *delay_on + *delay_off;
        // PSC = (period_seconds * 152) - 1
        let psc = ((total_ms as u32 * 152) / 1000).saturating_sub(1).min(255) as u8;
        // PWM = (delay_on / total_ms) * 256
        let pwm = ((*delay_on as u32 * 256) / total_ms as u32).min(255) as u8;

        self.write_reg(PCA9532_REG_PSC1, psc)?;
        self.write_reg(PCA9532_REG_PWM1, pwm)?;

        // Set LED0 to PWM1 blinker.
        let current_ls0 = self.read_reg(PCA9532_REG_LS0).unwrap_or(0);
        let new_ls0 = (current_ls0 & !0x03) | PCA9532_LED_PWM1;
        self.write_reg(PCA9532_REG_LS0, new_ls0)?;

        Ok(())
    }
}

/// The PCA9532 I2C LED driver structure.
struct Pca9532Driver;

impl i2c::Driver for Pca9532Driver {
    type IdInfo = ChipKind;
    type Data<'bound> = Pca9532Data;

    const I2C_ID_TABLE: Option<i2c::IdTable<Self::IdInfo>> = Some(&I2C_TABLE);
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        dev: &'bound i2c::I2cClient<device::Core<'_>>,
        id_info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        let kind = id_info.copied().unwrap_or(ChipKind::Pca9532);
        let raw_client = dev.as_raw();

        pr_info!("PCA953x Rust LED driver probing device at I2C adapter\n");

        let mut data = Pca9532Data {
            kind,
            raw_client,
            led_reg: None,
        };

        // Register LED class device with kernel.
        match leds::Registration::register(
            dev.as_ref(),
            c"pca9532:red:status",
            &data,
            255,
        ) {
            Ok(reg) => {
                pr_info!("PCA953x Rust: registered LED class device\n");
                data.led_reg = Some(reg);
            }
            Err(e) => {
                pr_warn!("PCA953x Rust: failed to register LED device: {:?}\n", e);
            }
        }

        Ok(data)
    }
}

kernel::module_i2c_driver! {
    type: Pca9532Driver,
    name: "leds_pca9532_rust",
    authors: ["Rust for Linux Developers"],
    description: "Rust NXP PCA9532/PCA9533 LED Dimmer Driver",
    license: "GPL",
}
