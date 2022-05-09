// SPDX-License-Identifier: GPL-2.0

//! Rust netfilter sample.

use kernel::net;
use kernel::net::filter::{self as netfilter, inet, Disposition, Family};
use kernel::prelude::*;

module! {
    type: RustNetfilter,
    name: b"rust_netfilter",
    author: b"Rust for Linux Contributors",
    description: b"Rust netfilter sample",
    license: b"GPL v2",
}

struct RustNetfilter {
    _in: Pin<Box<netfilter::Registration<Self>>>,
    _out: Pin<Box<netfilter::Registration<Self>>>,
}

impl netfilter::Filter for RustNetfilter {
    fn filter(_: (), skb: &net::SkBuff) -> Disposition {
        let data = skb.head_data();
        pr_info!(
            "packet headlen={}, len={}, first bytes={:02x?}\n",
            data.len(),
            skb.len(),
            &data[..core::cmp::min(10, data.len())]
        );
        Disposition::Accept
    }
}

impl kernel::Module for RustNetfilter {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        Ok(Self {
            _in: netfilter::Registration::new_pinned(
                Family::INet(inet::Hook::PreRouting),
                0,
                net::init_ns().into(),
                None,
                (),
            )?,
            _out: netfilter::Registration::new_pinned(
                Family::INet(inet::Hook::PostRouting),
                0,
                net::init_ns().into(),
                None,
                (),
            )?,
        })
    }
}
