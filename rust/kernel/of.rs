// SPDX-License-Identifier: GPL-2.0

//! Open Firmware abstractions.
//!
//! C header: [`include/linux/of_*.h`](srctree/include/linux/of_*.h)

use crate::{bindings, device_id::RawDeviceId, prelude::*};

/// An open firmware device id.
#[derive(Clone, Copy)]
pub struct DeviceId(bindings::of_device_id);

// SAFETY:
// * `DeviceId` is a `#[repr(transparent)` wrapper of `struct of_device_id` and does not add
//   additional invariants, so it's safe to transmute to `RawType`.
// * `DRIVER_DATA_OFFSET` is the offset to the `data` field.
unsafe impl RawDeviceId for DeviceId {
    type RawType = bindings::of_device_id;

    const DRIVER_DATA_OFFSET: usize = core::mem::offset_of!(bindings::of_device_id, data);

    fn index(&self) -> usize {
        self.0.data as _
    }
}

impl DeviceId {
    /// Create a new device id from an OF 'compatible' string.
    pub const fn new(compatible: &'static CStr) -> Self {
        let src = compatible.as_bytes_with_nul();
        // Replace with `bindings::of_device_id::default()` once stabilized for `const`.
        // SAFETY: FFI type is valid to be zero-initialized.
        let mut of: bindings::of_device_id = unsafe { core::mem::zeroed() };

        let mut i = 0;
        while i < src.len() {
            of.compatible[i] = src[i] as _;
            i += 1;
        }

        Self(of)
    }

    /// The compatible string of the embedded `struct bindings::of_device_id` as `&CStr`.
    pub fn compatible<'a>(&self) -> &'a CStr {
        // SAFETY: `self.compatible` is a valid `char` pointer.
        unsafe { CStr::from_char_ptr(self.0.compatible.as_ptr()) }
    }
}

/// Create an OF `IdTable` with an "alias" for modpost.
#[macro_export]
macro_rules! of_device_table {
    ($table_name:ident, $module_table_name:ident, $id_info_type: ty, $table_data: expr) => {
        const $table_name: $crate::device_id::IdArray<
            $crate::of::DeviceId,
            $id_info_type,
            { $table_data.len() },
        > = $crate::device_id::IdArray::new($table_data);

        $crate::module_device_table!("of", $module_table_name, $table_name);
    };
}
