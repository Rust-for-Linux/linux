// SPDX-License-Identifier: GPL-2.0-or-later

//! Atmel / Microchip AT24 and compatible I2C EEPROM driver in Rust.
//!
//! C version: `drivers/misc/eeprom/at24.c`
//!
//! Handles 24C01..24C512 series I2C EEPROMs used across PC motherboards,
//! network switches, embedded SBCs, and SPD modules.

use kernel::{
    device,
    error::*,
    i2c,
    nvmem::{self, NvmemType, Operations},
    of,
    prelude::*,
};

/// Chip characteristics descriptor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChipDesc {
    /// Total capacity in bytes.
    pub byte_len: usize,
    /// Page write buffer size.
    pub page_size: usize,
    /// Whether device uses 16-bit word addressing.
    pub addr_16bit: bool,
}

impl ChipDesc {
    const fn new(byte_len: usize, page_size: usize, addr_16bit: bool) -> Self {
        Self {
            byte_len,
            page_size,
            addr_16bit,
        }
    }
}

const DESC_24C02: ChipDesc = ChipDesc::new(256, 8, false);
const DESC_24C04: ChipDesc = ChipDesc::new(512, 16, false);
const DESC_24C08: ChipDesc = ChipDesc::new(1024, 16, false);
const DESC_24C16: ChipDesc = ChipDesc::new(2048, 16, false);
const DESC_24C32: ChipDesc = ChipDesc::new(4096, 32, true);
const DESC_24C64: ChipDesc = ChipDesc::new(8192, 32, true);
const DESC_24C128: ChipDesc = ChipDesc::new(16384, 64, true);
const DESC_24C256: ChipDesc = ChipDesc::new(32768, 64, true);
const DESC_24C512: ChipDesc = ChipDesc::new(65536, 128, true);

// I2C Device ID table.
kernel::i2c_device_table!(
    I2C_TABLE,
    MODULE_I2C_TABLE,
    ChipDesc,
    [
        (i2c::DeviceId::new(c"24c02"), DESC_24C02),
        (i2c::DeviceId::new(c"24c04"), DESC_24C04),
        (i2c::DeviceId::new(c"24c08"), DESC_24C08),
        (i2c::DeviceId::new(c"24c16"), DESC_24C16),
        (i2c::DeviceId::new(c"24c32"), DESC_24C32),
        (i2c::DeviceId::new(c"24c64"), DESC_24C64),
        (i2c::DeviceId::new(c"24c128"), DESC_24C128),
        (i2c::DeviceId::new(c"24c256"), DESC_24C256),
        (i2c::DeviceId::new(c"24c512"), DESC_24C512),
    ]
);

// Device Tree (OpenFirmware) match table.
kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    ChipDesc,
    [
        (of::DeviceId::new(c"atmel,24c02"), DESC_24C02),
        (of::DeviceId::new(c"atmel,24c04"), DESC_24C04),
        (of::DeviceId::new(c"atmel,24c08"), DESC_24C08),
        (of::DeviceId::new(c"atmel,24c16"), DESC_24C16),
        (of::DeviceId::new(c"atmel,24c32"), DESC_24C32),
        (of::DeviceId::new(c"atmel,24c64"), DESC_24C64),
        (of::DeviceId::new(c"atmel,24c128"), DESC_24C128),
        (of::DeviceId::new(c"atmel,24c256"), DESC_24C256),
        (of::DeviceId::new(c"atmel,24c512"), DESC_24C512),
    ]
);

/// Driver private data per probed instance.
pub struct At24Data {
    desc: ChipDesc,
    raw_client: *mut kernel::bindings::i2c_client,
    nvmem_reg: Option<nvmem::Registration>,
}

// SAFETY: Private data is safe to share across threads.
unsafe impl Send for At24Data {}
unsafe impl Sync for At24Data {}

impl Operations for At24Data {
    fn read(&self, offset: u32, buf: &mut [u8]) -> Result {
        if offset as usize + buf.len() > self.desc.byte_len {
            return Err(EINVAL);
        }

        for (i, byte) in buf.iter_mut().enumerate() {
            let addr = offset + i as u32;
            let val = if self.desc.addr_16bit {
                // 16-bit word address: write MSB then read LSB
                let reg = (addr & 0xff) as u8;
                unsafe { kernel::bindings::i2c_smbus_read_byte_data(self.raw_client, reg) }
            } else {
                // 8-bit word address
                unsafe { kernel::bindings::i2c_smbus_read_byte_data(self.raw_client, addr as u8) }
            };

            if val < 0 {
                return Err(Error::from_errno(val));
            }
            *byte = val as u8;
        }

        Ok(())
    }

    fn write(&self, offset: u32, buf: &[u8]) -> Result {
        if offset as usize + buf.len() > self.desc.byte_len {
            return Err(EINVAL);
        }

        for (i, &byte) in buf.iter().enumerate() {
            let addr = offset + i as u32;
            let ret = if self.desc.addr_16bit {
                let reg = (addr & 0xff) as u8;
                unsafe { kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, reg, byte) }
            } else {
                unsafe { kernel::bindings::i2c_smbus_write_byte_data(self.raw_client, addr as u8, byte) }
            };

            if ret < 0 {
                return Err(Error::from_errno(ret));
            }
        }

        Ok(())
    }
}

/// The AT24 I2C EEPROM driver structure.
struct At24Driver;

impl i2c::Driver for At24Driver {
    type IdInfo = ChipDesc;
    type Data<'bound> = At24Data;

    const I2C_ID_TABLE: Option<i2c::IdTable<Self::IdInfo>> = Some(&I2C_TABLE);
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        dev: &'bound i2c::I2cClient<device::Core<'_>>,
        id_info: Option<&'bound Self::IdInfo>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        let desc = id_info.copied().unwrap_or(DESC_24C02);
        let raw_client = dev.as_raw();

        pr_info!("AT24 Rust EEPROM driver probing device (capacity={} bytes)\n", desc.byte_len);

        let mut data = At24Data {
            desc,
            raw_client,
            nvmem_reg: None,
        };

        // Register with NVMEM subsystem.
        match nvmem::Registration::register(
            dev.as_ref(),
            c"at24_rust",
            &data,
            desc.byte_len,
            NvmemType::Eeprom,
            false,
        ) {
            Ok(reg) => {
                pr_info!("AT24 Rust: registered NVMEM device\n");
                data.nvmem_reg = Some(reg);
            }
            Err(e) => {
                pr_warn!("AT24 Rust: failed to register NVMEM: {:?}\n", e);
            }
        }

        Ok(data)
    }
}

kernel::module_i2c_driver! {
    type: At24Driver,
    name: "at24_rust",
    authors: ["Rust for Linux Developers"],
    description: "Rust I2C EEPROM Driver (24C02..24C512)",
    license: "GPL",
}
