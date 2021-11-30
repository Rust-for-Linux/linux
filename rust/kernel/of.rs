// SPDX-License-Identifier: GPL-2.0

//! Devicetree and Open Firmware abstractions.
//!
//! C header: [`include/linux/of_*.h`](../../../../include/linux/of_*.h)

use crate::{bindings, driver, str::BStr};

/// An open firmware device id.
#[derive(Clone, Copy)]
pub enum DeviceId {
    /// An open firmware device id where only a compatible string is specified.
    Compatible(&'static BStr),
}

/// Defines a const open firmware device id table that also carries per-entry data/context/info.
///
/// The name of the const is `OF_DEVICE_ID_TABLE`, which is what buses are expected to name their
/// open firmware tables.
///
/// # Examples
///
/// ```
/// # use kernel::define_of_id_table;
/// # use kernel::b_str;
/// # use kernel::str::BStr;
/// use kernel::of;
///
/// define_of_id_table! {u32, [
///     (of::DeviceId::Compatible(b_str!("test-device1,test-device2")), Some(0xff)),
///     (of::DeviceId::Compatible(b_str!("test-device3")), None),
/// ]};
/// ```
#[macro_export]
macro_rules! define_of_id_table {
    ($data_type:ty, $($t:tt)*) => {
        $crate::define_id_table!(OF_DEVICE_ID_TABLE, $crate::of::DeviceId, $data_type, $($t)*);
    };
}

// SAFETY: `ZERO` is all zeroed-out and `to_rawid` stores `offset` in `of_device_id::data`.
unsafe impl const driver::RawDeviceId for DeviceId {
    type RawType = bindings::of_device_id;
    const ZERO: Self::RawType = bindings::of_device_id {
        name: [0; 32],
        type_: [0; 32],
        compatible: [0; 128],
        data: core::ptr::null(),
    };

    fn to_rawid(&self, offset: isize) -> Self::RawType {
        let DeviceId::Compatible(compatible) = self;
        let mut id = Self::ZERO;
        let mut i = 0;
        while i < compatible.len() {
            // If `compatible` does not fit in `id.compatible`, an "index out of bounds" build time
            // error will be triggered.
            id.compatible[i] = compatible.as_bytes()[i] as _;
            i += 1;
        }
        id.compatible[i] = b'\0' as _;
        id.data = offset as _;
        id
    }
}
