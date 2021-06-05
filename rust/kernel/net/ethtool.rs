// SPDX-License-Identifier: GPL-2.0

//! Ethtool Operations.
//!
//! C header: [`include/linux/ethtool.h`](../../../../include/linux/ethtool.h)

use core::marker;

use crate::bindings;
use crate::from_kernel_result;
use crate::c_types;
use crate::error::{Error, Result};
use crate::types::{SavedAsPointer, SavedAsPointerMut};

use super::device::{NetDevice, NetDeviceAdapter};

unsafe extern "C" fn get_drvinfo_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
    info: *mut bindings::ethtool_drvinfo,
) {
    T::EthOps::get_drvinfo(
        unsafe { &NetDevice::<T>::from_pointer(dev) },
        unsafe { &mut EthtoolDrvinfo::from_pointer(info) },
    );
}

unsafe extern "C" fn get_ts_info_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
    info: *mut bindings::ethtool_ts_info,
) -> c_types::c_int {
    from_kernel_result! {
        T::EthOps::get_ts_info(
            // SAFETY: dev is valid, as this is a callback
            unsafe { &NetDevice::<T>::from_pointer(dev) },
            // SAFETY: info is valid, as this is a callback
            unsafe { &mut EthToolTsInfo::from_pointer(info) }
        )?;
        Ok(0)
    }
}

pub(crate) struct EthToolOperationsVtable<T: NetDeviceAdapter>(marker::PhantomData<T>);

impl<T: NetDeviceAdapter> EthToolOperationsVtable<T> {
    const VTABLE: bindings::ethtool_ops = bindings::ethtool_ops {
        _bitfield_align_1: [],
        _bitfield_1: bindings::__BindgenBitfieldUnit::<[u8; 1usize]>::new([0u8; 1usize]),
        supported_coalesce_params: 0,
        get_drvinfo: if T::EthOps::TO_USE.get_drvinfo {
            Some(get_drvinfo_callback::<T>)
        } else {
            None
        },
        get_regs_len: None,
        get_regs: None,
        get_wol: None,
        set_wol: None,
        get_msglevel: None,
        set_msglevel: None,
        nway_reset: None,
        get_link: None,
        get_link_ext_state: None,
        get_eeprom_len: None,
        get_eeprom: None,
        set_eeprom: None,
        get_coalesce: None,
        set_coalesce: None,
        get_ringparam: None,
        set_ringparam: None,
        get_pause_stats: None,
        get_pauseparam: None,
        set_pauseparam: None,
        self_test: None,
        get_strings: None,
        set_phys_id: None,
        get_ethtool_stats: None,
        begin: None,
        complete: None,
        get_priv_flags: None,
        set_priv_flags: None,
        get_sset_count: None,
        get_rxnfc: None,
        set_rxnfc: None,
        flash_device: None,
        reset: None,
        get_rxfh_key_size: None,
        get_rxfh_indir_size: None,
        get_rxfh: None,
        set_rxfh: None,
        get_rxfh_context: None,
        set_rxfh_context: None,
        get_channels: None,
        set_channels: None,
        get_dump_flag: None,
        get_dump_data: None,
        set_dump: None,
        get_ts_info: if T::EthOps::TO_USE.get_ts_info {
            Some(get_ts_info_callback::<T>)
        } else {
            None
        },
        get_module_info: None,
        get_module_eeprom: None,
        get_eee: None,
        set_eee: None,
        get_tunable: None,
        set_tunable: None,
        get_per_queue_coalesce: None,
        set_per_queue_coalesce: None,
        get_link_ksettings: None,
        set_link_ksettings: None,
        get_fecparam: None,
        set_fecparam: None,
        get_ethtool_phy_stats: None,
        get_phy_tunable: None,
        set_phy_tunable: None,
    };

    /// Builds an instance of [`struct ethtool_ops`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) const unsafe fn build() -> &'static bindings::ethtool_ops {
        &Self::VTABLE
    }
}

/// Represents which fields of [`struct ethtool_ops`] should pe populated with pointers for the trait [`EthToolOps`]
pub struct EthToolToUse {
    /// Trait defines a `get_drvinfo` function.
    pub get_drvinfo: bool,

    /// Trait defines a `get_ts_info` function.
    pub get_ts_info: bool,
}

/// This trait does not include any functions.
#[doc(hidden)]
pub const ETH_TOOL_USE_NONE: EthToolToUse = EthToolToUse {
    get_drvinfo: false,
    get_ts_info: false,
};

/// Defines the [`EthToolOps::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_eth_tool_ops {
    () => {
        const TO_USE: $crate::net::ethtool::EthToolToUse = $crate::net::ethtool::ETH_TOOL_USE_NONE;
    };
    ($($i:ident),+) => {
        const TO_USE: kernel::net::ethtool::EthToolToUse =
            $crate::net::ethtool::EthToolToUse {
                $($i: true),+ ,
                ..$crate::net::ethtool::ETH_TOOL_USE_NONE
            };
    };
}

/// Operations table needed for ethtool.
/// [`Self::TO_USE`] defines which functions are implemented by this type.
pub trait EthToolOps<T: NetDeviceAdapter>: Send + Sync + Sized {
    /// Struct [`EthToolToUse`] which signals which function are ipmlemeted by this type.
    const TO_USE: EthToolToUse;

    /// Report driver/device information.  Should only set the
    /// @driver, @version, @fw_version and @bus_info fields.  If not
    /// implemented, the @driver and @bus_info fields will be filled in
    /// according to the netdev's parent device.
    fn get_drvinfo(_dev: &NetDevice<T>, _info: &mut EthtoolDrvinfo) {}

    /// Get the time stamping and PTP hardware clock capabilities.
    /// Drivers supporting transmit time stamps in software should set this to
    /// [`helpers::ethtool_op_get_ts_info`].
    fn get_ts_info(_dev: &NetDevice<T>, _info: &mut EthToolTsInfo) -> Result {
        Err(Error::EINVAL)
    }
}

/// Wrappes the [`bindings::ethtool_ts_info`] struct.
#[repr(transparent)]
pub struct EthToolTsInfo {
    ptr: *const bindings::ethtool_ts_info,
}

impl SavedAsPointer for EthToolTsInfo {
    type InternalType = bindings::ethtool_ts_info;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}

impl SavedAsPointerMut for EthToolTsInfo {}

/// Wrappes the [`bindings::ethtool_drvinfo`] struct.
pub struct EthtoolDrvinfo {
    ptr: *const bindings::ethtool_drvinfo,
}

impl SavedAsPointer for EthtoolDrvinfo {
    type InternalType = bindings::ethtool_drvinfo;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self { ptr }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}

impl SavedAsPointerMut for EthtoolDrvinfo {}

/// Helper functions for ethtool.
pub mod helpers {
    use super::*;

    /// Get ts info for the device `dev`.
    pub fn ethtool_op_get_ts_info<T: NetDeviceAdapter>(
        dev: &NetDevice<T>,
        info: &mut EthToolTsInfo,
    ) -> Result {
        // SAFETY: dev.ptr is valid if dev is valid
        // SAFETY: info.ptr is valid if info is valid
        unsafe { bindings::ethtool_op_get_ts_info(dev.get_pointer_mut(), info.get_pointer_mut()) };
        Ok(())
    }
}
