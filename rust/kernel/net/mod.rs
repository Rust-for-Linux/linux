// SPDX-License-Identifier: GPL-2.0

//! Network subsystem in rust for the linux kernel.

use core::mem;

pub mod device;
pub mod ethtool;
pub mod netlink;
pub mod rtnl;
pub mod skbuff;

#[doc(inline)]
pub use macros::rtnl_link_ops;

/// Determine if the Ethernet address is a multicast.
///
/// Return true if the address is a multicast address.
/// By definition the broadcast address is also a multicast address.
///
/// # Parameters
/// - `addr`: Pointer to a six-byte array containing the Ethernet address
#[cfg(CONFIG_HAVE_EFFICIENT_UNALIGNED_ACCESS)]
pub unsafe fn is_multicast_ether_addr(addr: *const u8) -> bool {
    let a: u32 = unsafe { *(addr as *const u32) };

    if cfg!(target_endian = "big") {
        (0x01 & (a >> (((mem::size_of::<u32>() as u32) * 8) - 8))) != 0
    } else {
        (0x01 & a) != 0
    }
}

/// Determine if the Ethernet address is a multicast.
///
/// Return true if the address is a multicast address.
/// By definition the broadcast address is also a multicast address.
///
/// # Parameters
/// - `addr`: Pointer to a six-byte array containing the Ethernet address
#[cfg(not(CONFIG_HAVE_EFFICIENT_UNALIGNED_ACCESS))]
pub unsafe fn is_multicast_ether_addr(addr: *const u8) -> bool {
    let a: u16 = *(addr as *const u16);

    if cfg!(target_endian = "big") {
        (0x01 & (a >> (((mem::size_of::<u16>() as u16) * 8) - 8))) != 0
    } else {
        (0x01 & a) != 0
    }
}

/// Determine if give Ethernet address is all zeros.
/// Return true if the address is all zeroes.
///
/// # Parameters
/// - `addr`: Pointer to a six-byte array containing the Ethernet address
///
/// # Safety
/// Please note: addr must be aligned to u16.
#[cfg(CONFIG_HAVE_EFFICIENT_UNALIGNED_ACCESS)]
pub unsafe fn is_zero_ether_addr(addr: *const u8) -> bool {
    // SAFETY: function already unsafe
    unsafe { *(addr as *const u32) | (*((addr as usize + 4) as *const u16) as u32) == 0 }
}

/// Determine if give Ethernet address is all zeros.
/// Return true if the address is all zeroes.
///
/// # Parameters
/// - `addr`: Pointer to a six-byte array containing the Ethernet address
///
/// # Safety
/// Please note: addr must be aligned to u16.
#[cfg(not(CONFIG_HAVE_EFFICIENT_UNALIGNED_ACCESS))]
pub unsafe fn is_zero_ether_addr(addr: *const u8) -> bool {
    *(addr as *const u16)
        | *((addr as usize + 2) as *const u16)
        | *((addr as usize + 4) as *const u16)
        == 0
}

///  is_valid_ether_addr - Determine if the given Ethernet address is valid.
/// Check that the Ethernet address (MAC) is not 00:00:00:00:00:00, is not
/// a multicast address, and is not FF:FF:FF:FF:FF:FF.
///
/// Return true if the address is valid.
///
/// # Parameters
/// - `addr`: Pointer to a six-byte array containing the Ethernet address
///
/// # Safety
/// Please note: addr must be aligned to u16.
pub unsafe fn is_valid_ether_addr(addr: *const u8) -> bool {
    // SAFETY: function already unsafe
    unsafe { !is_multicast_ether_addr(addr) && !is_zero_ether_addr(addr) }
}

/// Prelude for all net related imports.
pub mod prelude {
    pub use super::rtnl_link_ops;
    pub use super::{
        device::{NetDevice, NetDeviceAdapter, NetDeviceOps},
        ethtool::{self, EthToolOps},
        rtnl::{RtnlLinkOps, RtnlLock},
        skbuff::SkBuff,
    };
}
