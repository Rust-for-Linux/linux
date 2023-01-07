// SPDX-License-Identifier: GPL-2.0

//! Broadcom BCM2835 Random Number Generator support.

use kernel::{
    device, file, file::File, io_buffer::IoBufferWriter, miscdev, module_platform_driver, of,
    platform, prelude::*, sync::Arc,
};

module_platform_driver! {
    type: RngDriver,
    name: "bcm2835_rng_rust",
    authors: ["Rust for Linux Contributors"],
    description: "BCM2835 Random Number Generator (RNG) driver",
    license: "GPL v2",
}

struct RngDevice;

#[vtable]
impl file::Operations for RngDevice {
    fn open(_open_data: &(), _file: &File) -> Result {
        Ok(())
    }

    fn read(_: (), _: &File, data: &mut impl IoBufferWriter, offset: u64) -> Result<usize> {
        // Succeed if the caller doesn't provide a buffer or if not at the start.
        if data.is_empty() || offset != 0 {
            return Ok(0);
        }

        data.write(&0_u32)?;
        Ok(4)
    }
}

type DeviceData = device::Data<miscdev::Registration<RngDevice>, (), ()>;

struct RngDriver;
impl platform::Driver for RngDriver {
    type Data = Arc<DeviceData>;

    kernel::define_of_id_table! {(), [
        (of::DeviceId::Compatible(b"brcm,bcm2835-rng"), None),
    ]}

    fn probe(dev: &mut platform::Device, _id_info: Option<&Self::IdInfo>) -> Result<Self::Data> {
        pr_info!("probing discovered hwrng with id {}\n", dev.id());
        let data = kernel::new_device_data!(
            miscdev::Registration::new(),
            (),
            (),
            "BCM2835::Registrations"
        )?;

        data.registrations()
            .ok_or(ENXIO)?
            .as_pinned_mut()
            .register(fmt!("rust_hwrng"), ())?;
        Ok(data.into())
    }
}
