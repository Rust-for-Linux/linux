// SPDX-License-Identifier: GPL-2.0

//! Devicetree and Open Firmware abstractions.
//!
//! C header: [`include/linux/of_*.h`](../../../../include/linux/of_*.h)

use crate::{
    bindings, device_id,
    str::{BStr, CStr},
};

/// An open firmware device id.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct DeviceId(bindings::of_device_id);

impl DeviceId {
    /// Create a OF `DeviceId` from a compatible string.
    pub const fn with_compatible(compatible: &CStr) -> Self {
        let device_id = core::mem::MaybeUninit::<bindings::of_device_id>::zeroed();
        let mut device_id = unsafe { device_id.assume_init() };

        let compatible = BStr::from_bytes(compatible.as_bytes_with_nul());
        assert!(compatible.len() <= device_id.compatible.len());

        let mut i = 0;
        while i < compatible.len() {
            device_id.compatible[i] = compatible.deref_const()[i] as _;
            i += 1;
        }

        Self(device_id)
    }
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores `offset` in `of_device_id::data`.
unsafe impl device_id::RawDeviceId for DeviceId {
    type RawType = bindings::of_device_id;
    const DRIVER_DATA_OFFSET: usize = core::mem::offset_of!(bindings::of_device_id, data);
}

/// Alias for `device_id::IdTable` containing OF's `DeviceId`
pub type IdTable<T> = &'static dyn device_id::IdTable<DeviceId, T>;

/// Create an OF `IdTable` with its alias for modpost.
#[macro_export]
macro_rules! of_device_table {
    ($module_table_name:ident, $table_name:ident, $id_info_type: ty, $table_data: expr) => {
        const $table_name: $crate::device_id::IdArray<
            $crate::of::DeviceId,
            $id_info_type,
            { $table_data.len() },
        > = $crate::device_id::IdArray::new($table_data);

        $crate::module_device_table!("of", $module_table_name, $table_name);
    };
}
