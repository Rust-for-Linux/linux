// SPDX-License-Identifier: GPL-2.0

//! Broadcom BCM2835 Random Number Generator support.

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::{
    file::File,
    file_operations::{FileOpener, FileOperations},
    io_buffer::IoBufferWriter,
    miscdev,
    of::ConstOfMatchTable,
    platdev::PlatformDriver,
    prelude::*,
    str::CStr,
    ThisModule, {c_str, platdev},
};

module! {
    type: RngModule,
    name: b"bcm2835_rng_rust",
    author: b"Rust for Linux Contributors",
    description: b"BCM2835 Random Number Generator (RNG) driver",
    license: b"GPL v2",
}

struct RngDevice;

impl FileOpener<()> for RngDevice {
    fn open(_state: &()) -> Result<Self::Wrapper> {
        Ok(Box::try_new(RngDevice)?)
    }
}

impl FileOperations for RngDevice {
    kernel::declare_file_operations!(read);

    fn read(_: &Self, _: &File, data: &mut impl IoBufferWriter, offset: u64) -> Result<usize> {
        // Succeed if the caller doesn't provide a buffer or if not at the start.
        if data.is_empty() || offset != 0 {
            return Ok(0);
        }

        data.write(&0_u32)?;
        Ok(4)
    }
}

struct RngDriver;

impl PlatformDriver for RngDriver {
    type DrvData = Pin<Box<miscdev::Registration<()>>>;

    fn probe(pdev: &mut PlatformDevice) -> Result<Self::DrvData> {
        pr_info!("probing discovered pdev with name {}\n", pdev.name());
        let drv_data =
            miscdev::Registration::new_pinned::<RngDevice>(c_str!("rust_hwrng"), None, ())?;
        Ok(drv_data)
    }

    fn remove(pdev: &mut PlatformDevice, _drv_data: Self::DrvData) -> Result {
        pr_info!("removing pdev with name {}\n", pdev.name());
        Ok(())
    }
}

struct RngModule {
    _pdev: Pin<Box<platdev::Registration>>,
}

impl KernelModule for RngModule {
    fn init(name: &'static CStr, module: &'static ThisModule) -> Result<Self> {
        const OF_MATCH_TBL: ConstOfMatchTable<1> =
            ConstOfMatchTable::new_const([c_str!("brcm,bcm2835-rng")]);

        let pdev =
            platdev::Registration::new_pinned::<RngDriver>(name, Some(&OF_MATCH_TBL), module)?;

        Ok(RngModule { _pdev: pdev })
    }
}
