// SPDX-License-Identifier: GPL-2.0

//! Netlink helpers.
//!
//! C header: [`include/linux/rtnetlink.h`](../../../../include/linux/rtnetlink.h)

use crate::bindings;
use crate::types::SavedAsPointer;

/// Octets in one ethernet addr
pub const ETH_ALEN: u16 = bindings::ETH_ALEN as u16;

const NLA_HDRLEN: i32 = bindings::BINDINGS_NLA_HDRLEN;
/// Max Position for NlAttrVec.
pub const __IFLA_MAX: usize = bindings::__IFLA_MAX as usize;

/// Wrapper for the kernels [`bindings::nlattr`] type.
#[repr(transparent)]
pub struct NlAttr {
    ptr: *const bindings::nlattr,
}

impl NlAttr {
    /// Check If the netlink is 0, This can happen in [`NlAttrVec`] structs.
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// Return the length of the nla data.
    /// Returns None if [`Self::is_null`] is true.
    pub fn nla_len(&self) -> Option<u16> {
        if self.is_null() {
            return None;
        }

        // NO-PANIC: self is valid and not null
        // SAFETY: ptr is valid if self is valid
        Some(self.get_internal().nla_len - NLA_HDRLEN as u16)
    }

    /// Get a pointer to the data inside the nla package.
    pub unsafe fn data(&self) -> *const i8 {
        ((self.ptr as usize) + NLA_HDRLEN as usize) as *const i8
    }

    /// Check if address inside the data is valid.
    pub fn is_valid_ether_addr(&self) -> bool {
        // SAFETY: self.o is valid if self is valid
        unsafe {
            let data = self.data() as *const u8;
            super::is_valid_ether_addr(data)
        }
    }

    /// Return a onwed version of Self.
    /// The pointer inside of [`Self::get_pointer`] still shows onto the same data.
    unsafe fn clone(&self) -> Self {
        Self {
            ptr: self.ptr.clone(),
        }
    }
}

impl SavedAsPointer for NlAttr {
    type InternalType = bindings::nlattr;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}

/// Wrapper for the kernels [`bindings::netlink_ext_ack`] stuct.
#[repr(transparent)]
pub struct NlExtAck {
    ptr: *const bindings::netlink_ext_ack,
}

impl SavedAsPointer for NlExtAck {
    type InternalType = bindings::netlink_ext_ack;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}

/// Wrapper for a list of [`NlAttr`] with size [`__IFLA_MAX`].
#[repr(transparent)]
pub struct NlAttrVec {
    ptr: *const *const bindings::nlattr,
}

impl NlAttrVec {
    /// Get The [`NlAttr`] from position `offset`.
    ///
    /// Returns None if `offset` is bigger than [`__IFLA_MAX`].
    pub fn get(&self, offset: u32) -> Option<NlAttr> {
        if offset > __IFLA_MAX as u32 {
            return None;
        }

        let vec = unsafe { &*(self.ptr as *const [NlAttr; __IFLA_MAX]) };
        let nlattr = &vec[offset as usize];
        if nlattr.is_null() {
            None
        } else {
            Some(unsafe { nlattr.clone() })
        }
    }
}

impl SavedAsPointer for NlAttrVec {
    type InternalType = *const bindings::nlattr;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}
