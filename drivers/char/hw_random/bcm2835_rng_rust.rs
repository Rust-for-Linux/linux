// SPDX-License-Identifier: GPL-2.0

//! Broadcom BCM2835 Random Number Generator support.

#![no_std]
#![feature(allocator_api, global_asm)]

use alloc::boxed::Box;
use core::pin::Pin;
use kernel::of::OfMatchTable;
use kernel::prelude::*;
use kernel::{c_str, platdev};

module! {
    type: RngModule,
    name: "bcm2835_rng_rust",
    author: "Rust for Linux Contributors",
    description: "BCM2835 Random Number Generator (RNG) driver",
    license: "GPL v2",
}

struct RngModule {
    _pdev: Pin<Box<platdev::Registration>>,
}

impl KernelModule for RngModule {
    fn init() -> Result<Self> {
        let of_match_tbl = OfMatchTable::new(&c_str!("brcm,bcm2835-rng"))?;

        let pdev = platdev::Registration::new_pinned(
            cstr!("bcm2835-rng-rust"),
            Some(of_match_tbl),
            &THIS_MODULE,
        )?;

        Ok(RngModule { _pdev: pdev })
    }
}
