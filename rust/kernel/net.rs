// SPDX-License-Identifier: GPL-2.0

//! Network subsystem.
//!
//! This module contains the kernel APIs related to networking that have been ported or wrapped in Rust.
//!
//! C header: [`include/linux/net.h`](../../../../include/linux/net.h) and related

use crate::error::{code, Error};
use core::cell::UnsafeCell;

#[cfg(CONFIG_RUST_PHYLIB_ABSTRACTIONS)]
pub mod phy;
pub mod addr;
pub mod ip;
pub mod socket;
pub mod tcp;
pub mod udp;

/// The address family.
///
/// See [`man 7 address families`](https://man7.org/linux/man-pages/man7/address_families.7.html) for more information.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AddressFamily {
    /// Unspecified address family.
    Unspec = bindings::AF_UNSPEC as isize,
    /// Local to host (pipes and file-domain).
    Unix = bindings::AF_UNIX as isize,
    /// Internetwork: UDP, TCP, etc.
    Inet = bindings::AF_INET as isize,
    /// Amateur radio AX.25.
    Ax25 = bindings::AF_AX25 as isize,
    /// IPX.
    Ipx = bindings::AF_IPX as isize,
    /// Appletalk DDP.
    Appletalk = bindings::AF_APPLETALK as isize,
    /// AX.25 packet layer protocol.
    Netrom = bindings::AF_NETROM as isize,
    /// Bridge link.
    Bridge = bindings::AF_BRIDGE as isize,
    /// ATM PVCs.
    Atmpvc = bindings::AF_ATMPVC as isize,
    /// X.25 (ISO-8208).
    X25 = bindings::AF_X25 as isize,
    /// IPv6.
    Inet6 = bindings::AF_INET6 as isize,
    /// ROSE protocol.
    Rose = bindings::AF_ROSE as isize,
    /// DECnet protocol.
    Decnet = bindings::AF_DECnet as isize,
    /// 802.2LLC project.
    Netbeui = bindings::AF_NETBEUI as isize,
    /// Firewall hooks.
    Security = bindings::AF_SECURITY as isize,
    /// Key management protocol.
    Key = bindings::AF_KEY as isize,
    /// Netlink.
    Netlink = bindings::AF_NETLINK as isize,
    /// Low-level packet interface.
    Packet = bindings::AF_PACKET as isize,
    /// Acorn Econet protocol.
    Econet = bindings::AF_ECONET as isize,
    /// ATM SVCs.
    Atmsvc = bindings::AF_ATMSVC as isize,
    /// RDS sockets.
    Rds = bindings::AF_RDS as isize,
    /// IRDA sockets.
    Irda = bindings::AF_IRDA as isize,
    /// Generic PPP.
    Pppox = bindings::AF_PPPOX as isize,
    /// Legacy WAN networks protocol.
    Wanpipe = bindings::AF_WANPIPE as isize,
    /// LLC protocol.
    Llc = bindings::AF_LLC as isize,
    /// Infiniband.
    Ib = bindings::AF_IB as isize,
    /// Multiprotocol label switching.
    Mpls = bindings::AF_MPLS as isize,
    /// Controller Area Network.
    Can = bindings::AF_CAN as isize,
    /// TIPC sockets.
    Tipc = bindings::AF_TIPC as isize,
    /// Bluetooth sockets.
    Bluetooth = bindings::AF_BLUETOOTH as isize,
    /// IUCV sockets.
    Iucv = bindings::AF_IUCV as isize,
    /// RxRPC sockets.
    Rxrpc = bindings::AF_RXRPC as isize,
    /// Modular ISDN protocol.
    Isdn = bindings::AF_ISDN as isize,
    /// Nokia cellular modem interface.
    Phonet = bindings::AF_PHONET as isize,
    /// IEEE 802.15.4 sockets.
    Ieee802154 = bindings::AF_IEEE802154 as isize,
    /// CAIF sockets.
    Caif = bindings::AF_CAIF as isize,
    /// Kernel crypto API
    Alg = bindings::AF_ALG as isize,
    /// VMware VSockets.
    Vsock = bindings::AF_VSOCK as isize,
    /// KCM sockets.
    Kcm = bindings::AF_KCM as isize,
    /// Qualcomm IPC router protocol.
    Qipcrtr = bindings::AF_QIPCRTR as isize,
    /// SMC sockets.
    Smc = bindings::AF_SMC as isize,
    /// Express Data Path sockets.
    Xdp = bindings::AF_XDP as isize,
}

impl From<AddressFamily> for isize {
    fn from(family: AddressFamily) -> Self {
        family as isize
    }
}

impl TryFrom<isize> for AddressFamily {
    type Error = Error;

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        let val = value as u32;
        match val {
            bindings::AF_UNSPEC => Ok(Self::Unspec),
            bindings::AF_UNIX => Ok(Self::Unix),
            bindings::AF_INET => Ok(Self::Inet),
            bindings::AF_AX25 => Ok(Self::Ax25),
            bindings::AF_IPX => Ok(Self::Ipx),
            bindings::AF_APPLETALK => Ok(Self::Appletalk),
            bindings::AF_NETROM => Ok(Self::Netrom),
            bindings::AF_BRIDGE => Ok(Self::Bridge),
            bindings::AF_ATMPVC => Ok(Self::Atmpvc),
            bindings::AF_X25 => Ok(Self::X25),
            bindings::AF_INET6 => Ok(Self::Inet6),
            bindings::AF_ROSE => Ok(Self::Rose),
            bindings::AF_DECnet => Ok(Self::Decnet),
            bindings::AF_NETBEUI => Ok(Self::Netbeui),
            bindings::AF_SECURITY => Ok(Self::Security),
            bindings::AF_KEY => Ok(Self::Key),
            bindings::AF_NETLINK => Ok(Self::Netlink),
            bindings::AF_PACKET => Ok(Self::Packet),
            bindings::AF_ECONET => Ok(Self::Econet),
            bindings::AF_ATMSVC => Ok(Self::Atmsvc),
            bindings::AF_RDS => Ok(Self::Rds),
            bindings::AF_IRDA => Ok(Self::Irda),
            bindings::AF_PPPOX => Ok(Self::Pppox),
            bindings::AF_WANPIPE => Ok(Self::Wanpipe),
            bindings::AF_LLC => Ok(Self::Llc),
            bindings::AF_IB => Ok(Self::Ib),
            bindings::AF_MPLS => Ok(Self::Mpls),
            bindings::AF_CAN => Ok(Self::Can),
            bindings::AF_TIPC => Ok(Self::Tipc),
            bindings::AF_BLUETOOTH => Ok(Self::Bluetooth),
            bindings::AF_IUCV => Ok(Self::Iucv),
            bindings::AF_RXRPC => Ok(Self::Rxrpc),
            bindings::AF_ISDN => Ok(Self::Isdn),
            bindings::AF_PHONET => Ok(Self::Phonet),
            bindings::AF_IEEE802154 => Ok(Self::Ieee802154),
            bindings::AF_CAIF => Ok(Self::Caif),
            bindings::AF_ALG => Ok(Self::Alg),
            bindings::AF_VSOCK => Ok(Self::Vsock),
            bindings::AF_KCM => Ok(Self::Kcm),
            bindings::AF_QIPCRTR => Ok(Self::Qipcrtr),
            bindings::AF_SMC => Ok(Self::Smc),
            bindings::AF_XDP => Ok(Self::Xdp),
            _ => Err(code::EINVAL),
        }
    }
}

/// Network namespace.
///
/// Wraps the `net` struct.
#[repr(transparent)]
pub struct Namespace(UnsafeCell<bindings::net>);

/// The global network namespace.
///
/// This is the default and initial namespace.
/// This function replaces the C `init_net` global variable.
pub fn init_net() -> &'static Namespace {
    // SAFETY: `init_net` is a global variable and is always valid.
    let ptr = unsafe { core::ptr::addr_of!(bindings::init_net) };
    // SAFETY: the address of `init_net` is always valid, always points to initialized memory,
    // and is always aligned.
    unsafe { &*(ptr.cast()) }
}
