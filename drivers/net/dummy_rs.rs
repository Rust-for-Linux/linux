// SPDX-License-Identifier: GPL-2.0

//! Rust dummy network driver
//!
//! This is a demonstration of what a small driver looks like in Rust, based on drivers/net/dummy.c.
//! This code is provided as a demonstration only, not as a proposal to mass-rewrite existing drivers in Rust
//!
//! The purpose of this driver is to provide a device to point a
//! route through, but not to actually transmit packets.
//!
//! Why?  If you have a machine whose only connection is an occasional
//! PPP/SLIP/PLIP link, you can only connect to your own hostname
//! when the link is up.  Otherwise you have to use localhost.
//! This isn't very consistent.
//!
//! One solution is to set up a dummy link using PPP/SLIP/PLIP,
//! but this seems (to me) too much overhead for too little gain.
//! This driver provides a small alternative. Thus you can do
//!
//! [when not running slip]
//! 	ifconfig dummy slip.addr.ess.here up
//! [to go to slip]
//! 	ifconfig dummy down
//! 	dip whatever
//!
//! This was written by looking at the dummy network driver from Nick
//! Holloway, which was written by looking at Donald Becker's skeleton driver
//! and the loopback driver.
//!
//!  Finn Behrens, 30th April 2021
//!
//! rust rewrite of the C version from Nick Holloway, 27th May 1994
//! see [dummy.c](./dummy.c)

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::net::device;
use kernel::net::prelude::*;
use kernel::net::rtnl;
use kernel::Error;
use kernel::{
    net::netlink::{NlAttrVec, NlExtAck},
    prelude::*,
};

module! {
    type: RustNetDummy,
    name: b"dummy_rs",
    author: b"Rust for Linux Contributors",
    description: b"Rust dummy network driver",
    license: b"GPL v2",
    alias_rtnl_link: b"dummy_rs",
    params: {
        numdummies: usize {
            default: 0,
            permissions: 0,
            description: b"Number of dummy_rs pseudo devices",
        },
    },
}

fn setup(dev: &mut NetDevice<DummyRsDev>) {
    dev.ether_setup();

    dev.set_ops();

    // Fill in device structure with ethernet-generic values.
    dev.add_flag(device::Iff::NOARP);
    dev.remove_flag(device::Iff::MULTICAST);

    dev.add_private_flag(device::IffPriv::LIVE_ADDR_CHANGE);
    dev.add_private_flag(device::IffPriv::NO_QUEUE);

    let mut feature = device::feature::NetIF::new();

    feature += device::feature::NETIF_F_SG;
    feature += device::feature::NETIF_F_FRAGLIST;
    feature += device::feature::NETIF_F_GSO_SOFTWARE;
    feature += device::feature::NETIF_F_HW_CSUM;
    feature += device::feature::NETIF_F_HIGHDMA;
    feature += device::feature::NETIF_F_LLTX;
    feature += device::feature::NETIF_F_GSO_ENCAP_ALL;

    dev.set_features(feature);
    dev.set_hw_features(feature);
    dev.set_hw_enc_features(feature);

    dev.hw_addr_random();
    dev.set_mtu(0, 0);
}

fn validate(tb: &NlAttrVec, _data: &NlAttrVec, _ext_ack: &NlExtAck) -> Result {
    if let Some(addr) = tb.get(kernel::bindings::IFLA_ADDRESS) {
        if Some(kernel::net::netlink::ETH_ALEN) != addr.nla_len() {
            return Err(Error::EINVAL);
        }
        if !addr.is_valid_ether_addr() {
            return Err(Error::EADDRNOTAVAIL);
        }
    }
    Ok(())
}

rtnl_link_ops! {
    kind: b"dummy_rs",
    type: DummyRsDev,
    setup: setup,
    validate: validate,
}

struct RustNetDummy {
    //dev: NetDevice<DummyRsDev>,
}

impl KernelModule for RustNetDummy {
    fn init() -> Result<Self> {
        let num = *numdummies.read();

        unsafe { dummy_rs_link_ops.register() }?;

        for _ in 0..(num) {
            let dev = NetDevice::new(
                DummyRsDev,
                kernel::c_str!("dummyrs%d"),
                kernel::net::device::NetNameAssingType::Enum,
                1,
                1,
            )?;
            dev.set_rtnl_ops(unsafe { &dummy_rs_link_ops });

            if let Err(e) = dev.register() {
                pr_warn!("could not register: {}", e.to_kernel_errno());
                return Err(e);
            }
        }

        Ok(RustNetDummy {
            //dev,
        })
    }
}

impl Drop for RustNetDummy {
    fn drop(&mut self) {
        // TODO: remove unsafe somehow
        unsafe { dummy_rs_link_ops.unregister() };
    }
}

struct DummyRsDev;

impl NetDeviceOps<Self> for DummyRsDev {
    kernel::declare_net_device_ops!(
        get_stats64,
        change_carrier,
        validate_addr,
        set_mac_addr,
        set_rx_mode
    );

    fn init(dev: &mut NetDevice<Self>) -> Result {
        dev.set_new_pcpu_lstats()?;
        Ok(())
    }

    fn uninit(dev: &mut NetDevice<Self>) {
        unsafe { dev.free_lstats() };
    }

    fn start_xmit(skb: SkBuff, dev: &mut NetDevice<Self>) -> kernel::net::device::NetdevTX {
        let mut skb = skb;

        dev.lstats_add(skb.len());

        skb.tx_timestamp();
        drop(skb);

        kernel::net::device::NetdevTX::TX_OK
    }

    fn get_stats64(dev: &NetDevice<Self>, stats: &mut rtnl::RtnlLinkStats64) {
        stats.dev_read(dev);
    }

    fn change_carrier(dev: &mut NetDevice<Self>, new_carrier: bool) -> Result {
        dev.carrier_set(new_carrier);

        Ok(())
    }

    fn validate_addr(dev: &NetDevice<Self>) -> Result {
        device::helpers::eth_validate_addr(dev)
    }

    fn set_mac_addr(dev: &mut NetDevice<Self>, p: *mut kernel::c_types::c_void) -> Result {
        device::helpers::eth_mac_addr(dev, p)
    }

    // [Someting about faking multicast](https://elixir.bootlin.com/linux/v5.12-rc4/source/drivers/net/dummy.c#L48).
    fn set_rx_mode(_dev: &mut NetDevice<Self>) {}
}

impl NetDeviceAdapter for DummyRsDev {
    type Inner = Self;

    type Ops = Self;

    type EthOps = Self;

    fn setup(dev: &mut NetDevice<Self>) {
        setup(dev);
    }
}

impl EthToolOps<Self> for DummyRsDev {
    kernel::declare_eth_tool_ops!(get_drvinfo, get_ts_info);

    fn get_drvinfo(_dev: &NetDevice<Self>, info: &mut ethtool::EthtoolDrvinfo) {
        // TODO: how to do this more efficient without unsafe?
        // FIXME: !!
        let info: &kernel::bindings::ethtool_drvinfo = info.get_internal();
        unsafe {
            kernel::bindings::strlcpy(
                &(info.driver) as *const _ as *mut i8,
                b"dummy_rs\0" as *const _ as *mut i8,
                32,
            );
        }
    }

    fn get_ts_info(dev: &NetDevice<Self>, info: &mut ethtool::EthToolTsInfo) -> Result {
        kernel::net::ethtool::helpers::ethtool_op_get_ts_info(dev, info)
    }
}
