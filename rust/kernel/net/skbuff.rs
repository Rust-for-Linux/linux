// SPDX-License-Identifier: GPL-2.0

//! Definitors for the struct [`crate::bindings::sk_buff`] memory handlers.

use core::{ops::Drop, ptr};

use crate::bindings;
use crate::types::SavedAsPointer;

/// Wraps the kernel's `struct sk_buff`.
///
/// # Invariants
///
/// The pointer `Self::ptr` is non-null and valid.
#[repr(transparent)]
pub struct SkBuff {
    ptr: *const bindings::sk_buff,
}

impl SkBuff {
    /// Binding to `bindings::skb_clone_tx_timestamp`.
    ///
    /// Noop if `CONFIG_NETWORK_PHY_TIMESTAMPING` is not enabled.
    #[cfg(CONFIG_NETWORK_PHY_TIMESTAMPING)]
    pub fn clone_tx_timestamp(&mut self) {
        // SAFETY: self.ptr is valid if self is valid.
        unsafe {
            bindings::skb_clone_tx_timestamp(self.ptr as *mut bindings::sk_buff);
        }
    }

    /// Binding to `bindings::skb_clone_tx_timestamp`.
    ///
    /// Noop if `CONFIG_NETWORK_PHY_TIMESTAMPING` is not enabled.
    #[cfg(not(CONFIG_NETWORK_PHY_TIMESTAMPING))]
    pub fn clone_tx_timestamp(&mut self) {
        // NOOP
    }

    /// Driver hook for transmit timestamping.
    ///
    /// Ethernet MAC Drivers should call this function in their hard_xmit()
    /// function immediately before giving the sk_buff to the MAC hardware.
    ///
    /// Specifically, one should make absolutely sure that this function is
    /// called before TX completion of this packet can trigger.  Otherwise
    /// the packet could potentially already be freed.
    pub fn tx_timestamp(&mut self) {
        self.clone_tx_timestamp();
        if self.shinfo().tx_flags() as u32 & bindings::SKBTX_SW_TSTAMP != 0 {
            unsafe {
                bindings::skb_tstamp_tx(self.ptr as *mut bindings::sk_buff, ptr::null_mut());
            }
        }
    }

    /// Length of the [`SkBuff`].
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u32 {
        self.get_internal().len
    }

    /// Get the Shared info for this SKBuffer.
    pub fn shinfo(&self) -> SkbSharedInfo {
        // SAFETY: self.ptr is valid if self is valid
        unsafe {
            let info = self.shinfo_int();
            SkbSharedInfo::from_pointer(info)
        }
    }

    unsafe fn shinfo_int(&self) -> *mut bindings::skb_shared_info {
        self.end_pointer() as *mut bindings::skb_shared_info
    }

    // NET_SKBUFF_DATA_USES_OFFSET
    #[cfg(target_pointer_width = "64")]
    fn end_pointer(&self) -> *mut u8 {
        let sk_ref = self.get_internal();
        (sk_ref.head as usize + sk_ref.end as usize) as *mut u8
    }

    // !NET_SKBUFF_DATA_USES_OFFSET
    #[cfg(not(target_pointer_width = "64"))]
    fn end_pointer(&self) -> *mut u8 {
        let sk_ref = self.get_internal();
        sk_ref.end as *mut u8
    }
}

impl SavedAsPointer for SkBuff {
    type InternalType = bindings::sk_buff;

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }
}

// TODO: is this sound?
impl Drop for SkBuff {
    #[cfg(CONFIG_TRACEPOINTS)]
    fn drop(&mut self) {
        // SAFETY: self.ptr is valid if self is valid
        unsafe {
            bindings::consume_skb(self.ptr as *mut bindings::sk_buff);
        }
    }

    #[cfg(not(CONFIG_TRACEPOINTS))]
    fn drop(&mut self) {
        // SAFETY: self.ptr is valid if self is valid
        unsafe {
            bindings::kfree_skb(self.ptr as *mut bindings::sk_buff);
        }
    }
}

/// Wraps the kernel's `struct skb_shared_info`.
///
/// # Invariants
///
/// The pointer `Self::ptr` is non-null and valid.
#[repr(transparent)]
pub struct SkbSharedInfo {
    ptr: *const bindings::skb_shared_info,
}

impl SkbSharedInfo {
    /// The `tx_flag` field of the wrapped [`bindings::skb_shared_info`]
    pub fn tx_flags(&self) -> u8 {
        self.get_internal().tx_flags
    }
}

impl SavedAsPointer for SkbSharedInfo {
    type InternalType = bindings::skb_shared_info;

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }
}
