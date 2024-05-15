// SPDX-License-Identifier: GPL-2.0

use alloc::boxed::Box;
use core::pin::Pin;
use kernel::c_str;
use kernel::prelude::*;
use kernel::spi::*;

module! {
    type: SPIDummy,
    name: "rust_spi_dummy",
    author: "ks0n",
    description: "SPI Dummy Driver",
    license: "GPL",
}

struct SPIDummyMethods;

#[vtable]
impl SpiMethods for SPIDummyMethods {
    fn probe(spi_device: &mut SpiDevice) -> Result<i32, Error> {
        pr_info!("[SPI-RS] SPI Registered\n");
        pr_info!(
            "[SPI-RS] SPI Registered, spi_device = {:#?}\n",
            spi_device.to_ptr()
        );

        Ok(0)
    }
}

struct SPIDummy {
    _spi: Pin<Box<DriverRegistration<SPIDummyMethods>>>,
}

const TEST: u8 = 0;
const fn test() -> usize {
    42
}

static ID_TABLE: &[SpiDeviceId] = &[
    SpiDeviceId::new(c_str!("test1")).with_driver_data_pointer(&TEST),
    SpiDeviceId::new(c_str!("test2")).with_driver_data_pointer(&test),
    SpiDeviceId::new(c_str!("test3")).with_driver_data_number(42),
    SpiDeviceId::sentinel(),
];

impl kernel::Module for SPIDummy {
    fn init(module: &'static ThisModule) -> Result<Self> {
        pr_info!("[SPI-RS] Init\n");

        let spi = DriverRegistration::new_pinned(module, c_str!("SPIDummy"), Some(&ID_TABLE))?;

        Ok(SPIDummy { _spi: spi })
    }
}

impl Drop for SPIDummy {
    fn drop(&mut self) {
        pr_info!("[SPI-RS] Exit\n");
    }
}
