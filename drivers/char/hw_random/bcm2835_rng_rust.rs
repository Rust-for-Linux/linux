// SPDX-License-Identifier: GPL-2.0

//! Broadcom BCM2835 Random Number Generator support.

use kernel::{
    device, file::File, file_operations::FileOperations, io_buffer::IoBufferWriter, miscdev,
    module_platform_driver, of, platform, prelude::*, sync::Ref, str::BStr, b_str,
};

module_platform_driver! {
    type: RngDriver,
    name: b"bcm2835_rng_rust",
    author: b"Rust for Linux Contributors",
    description: b"BCM2835 Random Number Generator (RNG) driver",
    license: b"GPL v2",
}

struct RngDevice;

impl FileOperations for RngDevice {
    kernel::declare_file_operations!(read);

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
    type Data = Ref<DeviceData>;

    kernel::define_of_id_table! {(), [
        (of::DeviceId::Compatible(b_str!("brcm,bcm2835-rng")), None),
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
            .ok_or(Error::ENXIO)?
            .as_pinned_mut()
            .register(fmt!("rust_hwrng"), ())?;
        Ok(data.into())
    }
}
