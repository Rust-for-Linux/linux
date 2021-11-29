// SPDX-License-Identifier: GPL-2.0

//! RNG driver for Exynos TRNGs
//!
//! Author: Maciej Falkowski <m.falkowski@samsung.com>
//!
//! Copyright 2021 (c) Samsung Electronics Software, Inc.
//!
//! Based on the Exynos TRNG driver drivers/char/hw_random/exynos-rng by
//! Łukasz Stelmach <l.stelmach@samsung.com>

#![no_std]
#![feature(allocator_api, global_asm)]

use core::{cmp::min, convert::TryInto, str};

use kernel::{
    c_str,
    clk::{self, Clk},
    declare_hwrng_operations,
    device::RawDevice,
    hw_random::{self, HwrngOperations},
    io_mem::{self, IoMem},
    of::ConstOfMatchTable,
    platdev::{self, PlatformDevice, PlatformDriver},
    prelude::*,
};

module! {
    type: ExynosTrngDriverModule,
    name: b"exynos_trng_rust",
    author: b"Maciej Falkowski <m.falkowski@samsung.com>",
    description: b"RNG driver for Exynos TRNGs",
    license: b"GPL v2",
}

const EXYNOS_TRNG_CLKDIV: usize = 0x0;

const EXYNOS_TRNG_CTRL: usize = 0x20;
const EXYNOS_TRNG_CTRL_RNGEN: u32 = 0x1 << 31; // BIT(31)

const EXYNOS_REG_SIZE: usize = 0x100;
const EXYNOS_TRNG_POST_CTRL: usize = 0x30;
const _EXYNOS_TRNG_ONLINE_CTRL: usize = 0x40;
const _EXYNOS_TRNG_ONLINE_STAT: usize = 0x44;
const _EXYNOS_TRNG_ONLINE_MAXCHI2: usize = 0x48;
const EXYNOS_TRNG_FIFO_CTRL: usize = 0x50;
const EXYNOS_TRNG_FIFO_0: usize = 0x80;
const _EXYNOS_TRNG_FIFO_1: usize = 0x84;
const _EXYNOS_TRNG_FIFO_2: usize = 0x88;
const _EXYNOS_TRNG_FIFO_3: usize = 0x8c;
const _EXYNOS_TRNG_FIFO_4: usize = 0x90;
const _EXYNOS_TRNG_FIFO_5: usize = 0x94;
const _EXYNOS_TRNG_FIFO_6: usize = 0x98;
const _EXYNOS_TRNG_FIFO_7: usize = 0x9c;
const EXYNOS_TRNG_FIFO_LEN: u32 = 8;
const EXYNOS_TRNG_CLOCK_RATE: u32 = 500000;

struct ExynosTrngDevice {
    clk: Clk,
    mem: IoMem<EXYNOS_REG_SIZE>,
}

impl HwrngOperations for ExynosTrngDevice {
    declare_hwrng_operations!(init);

    type Data = Box<ExynosTrngDevice>;

    fn init(trng: &Self) -> Result {
        let sss_rate = trng.clk.get_rate();

        // For most TRNG circuits the clock frequency of under 500 kHz
        // is safe.
        let mut val = sss_rate / (EXYNOS_TRNG_CLOCK_RATE * 2);
        if val > 0x7fff {
            // dev_err(trng->dev, "clock divider too large: %d", val);
            pr_info!("rust_hwrng_init(): clock divider too large: {}\n", val);
            return Err(Error::ERANGE);
        }

        val <<= 1;
        trng.mem.writel_relaxed(val, EXYNOS_TRNG_CLKDIV);

        // Enable the generator.
        val = EXYNOS_TRNG_CTRL_RNGEN;
        trng.mem.writel_relaxed(val, EXYNOS_TRNG_CTRL);

        // Disable post-processing. /dev/hwrng is supposed to deliver
        // unprocessed data.
        trng.mem.writel_relaxed(0, EXYNOS_TRNG_POST_CTRL);

        Ok(())
    }

    fn read(trng: &Self, data: &mut [i8], _wait: bool) -> Result<i32> {
        let max: u32 = min(data.len().try_into()?, EXYNOS_TRNG_FIFO_LEN * 4);

        trng.mem.writel_relaxed(max * 8, EXYNOS_TRNG_FIFO_CTRL);

        let _ =
            trng.mem
                .readl_poll_timeout(EXYNOS_TRNG_FIFO_CTRL, |val| *val == 0, 200, 1000000)?;

        io_mem::try_memcpy_fromio(&trng.mem, data, EXYNOS_TRNG_FIFO_0, max.try_into()?)?;

        Ok(max as i32)
    }
}

struct ExynosTrngDriver;

impl PlatformDriver for ExynosTrngDriver {
    type DrvData = Pin<Box<hw_random::Registration<ExynosTrngDevice>>>;

    fn probe(pdev: &mut PlatformDevice) -> Result<Self::DrvData> {
        let mut clk = clk::devm_clk_get(pdev, c_str!("secss"))?;
        clk.prepare_enable()?;

        let mem = IoMem::<EXYNOS_REG_SIZE>::try_platform_ioremap_resource(pdev, 0)?;

        let exynos_trng = Box::try_new(ExynosTrngDevice { clk, mem })?;

        let drv_data = hw_random::Registration::new_pinned(pdev.name(), 0, exynos_trng)?;

        Ok(drv_data)
    }

    fn remove(_pdev: &mut PlatformDevice, _drv_data: Self::DrvData) -> Result {
        Ok(())
    }
}

struct ExynosTrngDriverModule {
    _pdev: Pin<Box<platdev::Registration>>,
}

impl KernelModule for ExynosTrngDriverModule {
    fn init(name: &'static CStr, module: &'static ThisModule) -> Result<Self> {
        const OF_MATCH_TBL: ConstOfMatchTable<1> =
            ConstOfMatchTable::new_const([c_str!("samsung,exynos5250-trng")]);

        let pdev = platdev::Registration::new_pinned::<ExynosTrngDriver>(
            name,
            Some(&OF_MATCH_TBL),
            module,
        )?;

        Ok(ExynosTrngDriverModule { _pdev: pdev })
    }
}
