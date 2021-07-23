// SPDX-License-Identifier: GPL-2.0

//! Rtnl Link Operations.

use core::ptr;

use crate::bindings;
use crate::error::{Error, Result};

use super::device::{NetDevice, NetDeviceAdapter};

use crate::types::{SavedAsPointer, SavedAsPointerMut};

// TODO: inner bool, to allow other unlock mechanism?
/// Lock, acquired via [`bindings::rtnl_lock`].
#[must_use = "the rtnl unlocks immediately when the guard is unused"]
pub struct RtnlLock {
    _private: (),
}

impl RtnlLock {
    /// Creat a new lock via [`bindings::rtnl_lock`].
    pub fn lock() -> Self {
        // SAFETY: C function without parameters
        unsafe { bindings::rtnl_lock() };

        Self { _private: () }
    }
}

impl Drop for RtnlLock {
    fn drop(&mut self) {
        // SAFETY: C function without parameters
        unsafe { bindings::rtnl_unlock() };
    }
}

#[doc(hidden)]
/// Empty [`bindings::rtnl_link_ops`] table.
pub const RTNL_LINK_OPS_EMPTY: bindings::rtnl_link_ops = bindings::rtnl_link_ops {
    list: bindings::list_head {
        next: ptr::null::<bindings::list_head>() as *mut bindings::list_head,
        prev: ptr::null::<bindings::list_head>() as *mut bindings::list_head,
    },
    kind: ptr::null::<i8>(),
    priv_size: 0,
    setup: None,
    netns_refund: false,
    maxtype: 0,
    policy: ptr::null::<bindings::nla_policy>(),
    validate: None,
    newlink: None,
    changelink: None,
    dellink: None,
    get_size: None,
    fill_info: None,
    get_xstats_size: None,
    fill_xstats: None,
    get_num_tx_queues: None,
    get_num_rx_queues: None,
    slave_maxtype: 0,
    slave_policy: ptr::null::<bindings::nla_policy>(),
    slave_changelink: None,
    get_slave_size: None,
    fill_slave_info: None,
    get_link_net: None,
    get_linkxstats_size: None,
    fill_linkxstats: None,
};

/// Transparent wrapper for [`bindings::rtnl_link_ops`] for the macro [`crate::net::rtnl_link_ops`].
#[repr(transparent)]
pub struct RtnlLinkOps(bindings::rtnl_link_ops);

unsafe impl Sync for RtnlLinkOps {}

impl RtnlLinkOps {
    /// Register this op table with kernel.
    pub fn register(&self) -> Result {
        let ptr = self.as_ptr();

        // SAFETY: Calling C function
        let ret = unsafe { bindings::rtnl_link_register(ptr as *mut bindings::rtnl_link_ops) };

        if ret != 0 {
            Err(Error::from_kernel_errno(ret))
        } else {
            Ok(())
        }
    }

    /// Get a pointer to this struct.
    pub fn as_ptr(&self) -> *const bindings::rtnl_link_ops {
        &self.0 as *const _ as *const bindings::rtnl_link_ops
    }

    /// Deregister the op table from the kernel.
    pub fn unregister(&self) {
        let ptr = self.as_ptr() as *mut bindings::rtnl_link_ops;

        // SAFETY: ptr is valid if self is valid
        unsafe { bindings::rtnl_link_unregister(ptr) };
    }

    #[doc(hidden)]
    /// Create an new unchecked version needed for the [`crate::net::rtnl_link_ops`] macro.
    pub const fn new_from_inner(inner: bindings::rtnl_link_ops) -> Self {
        Self(inner)
    }
}

/// Wrapper for a kernel [`bindings::rtnl_link_stats64`].
#[repr(transparent)]
pub struct RtnlLinkStats64 {
    ptr: *const bindings::rtnl_link_stats64,
}

impl RtnlLinkStats64 {
    /// Read the lstats from the [`NetDevice`] `dev` to [`Self`].
    pub fn dev_read<T: NetDeviceAdapter>(&mut self, dev: &mut NetDevice<T>) {
        let stats = self.get_internal_mut();
        // SAFETY: call to C function
        unsafe {
            bindings::dev_lstats_read(
                dev.get_pointer_mut(),
                &mut stats.tx_packets as *mut u64,
                &mut stats.tx_bytes as *mut u64,
            );
        }
    }
}

impl SavedAsPointer for RtnlLinkStats64 {
    type InternalType = bindings::rtnl_link_stats64;

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }
}

impl SavedAsPointerMut for RtnlLinkStats64 {
    unsafe fn from_pointer_mut(ptr: *mut Self::InternalType) -> Self {
        unsafe { Self::from_pointer(ptr as *const Self::InternalType) }
    }
}
