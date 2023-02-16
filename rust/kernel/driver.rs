// SPDX-License-Identifier: GPL-2.0

//! Generic support for drivers of different buses (e.g., PCI, Platform, Amba, etc.).
//!
//! Each bus/subsystem is expected to implement [`DriverOps`], which allows drivers to register
//! using the [`Registration`] class.

use crate::{error::code::*, error::Result, str::CStr, sync::Arc, ThisModule};
use alloc::boxed::Box;
use core::{cell::UnsafeCell, marker::PhantomData, ops::Deref, pin::Pin};

/// A subsystem (e.g., PCI, Platform, Amba, etc.) that allows drivers to be written for it.
pub trait DriverOps {
    /// The type that holds information about the registration. This is typically a struct defined
    /// by the C portion of the kernel.
    type RegType: Default;

    /// Registers a driver.
    ///
    /// # Safety
    ///
    /// `reg` must point to valid, initialised, and writable memory. It may be modified by this
    /// function to hold registration state.
    ///
    /// On success, `reg` must remain pinned and valid until the matching call to
    /// [`DriverOps::unregister`].
    unsafe fn register(
        reg: *mut Self::RegType,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result;

    /// Unregisters a driver previously registered with [`DriverOps::register`].
    ///
    /// # Safety
    ///
    /// `reg` must point to valid writable memory, initialised by a previous successful call to
    /// [`DriverOps::register`].
    unsafe fn unregister(reg: *mut Self::RegType);
}

/// The registration of a driver.
pub struct Registration<T: DriverOps> {
    is_registered: bool,
    concrete_reg: UnsafeCell<T::RegType>,
}

// SAFETY: `Registration` has no fields or methods accessible via `&Registration`, so it is safe to
// share references to it with multiple threads as nothing can be done.
unsafe impl<T: DriverOps> Sync for Registration<T> {}

impl<T: DriverOps> Registration<T> {
    /// Creates a new instance of the registration object.
    pub fn new() -> Self {
        Self {
            is_registered: false,
            concrete_reg: UnsafeCell::new(T::RegType::default()),
        }
    }

    /// Allocates a pinned registration object and registers it.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned(name: &'static CStr, module: &'static ThisModule) -> Result<Pin<Box<Self>>> {
        let mut reg = Pin::from(Box::try_new(Self::new())?);
        reg.as_mut().register(name, module)?;
        Ok(reg)
    }

    /// Registers a driver with its subsystem.
    ///
    /// It must be pinned because the memory block that represents the registration is potentially
    /// self-referential.
    pub fn register(
        self: Pin<&mut Self>,
        name: &'static CStr,
        module: &'static ThisModule,
    ) -> Result {
        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.is_registered {
            // Already registered.
            return Err(EINVAL);
        }

        // SAFETY: `concrete_reg` was initialised via its default constructor. It is only freed
        // after `Self::drop` is called, which first calls `T::unregister`.
        unsafe { T::register(this.concrete_reg.get(), name, module) }?;

        this.is_registered = true;
        Ok(())
    }
}

impl<T: DriverOps> Default for Registration<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DriverOps> Drop for Registration<T> {
    fn drop(&mut self) {
        if self.is_registered {
            // SAFETY: This path only runs if a previous call to `T::register` completed
            // successfully.
            unsafe { T::unregister(self.concrete_reg.get()) };
        }
    }
}

/// Conversion from a device id to a raw device id.
///
/// This is meant to be implemented by buses/subsystems so that they can use [`IdTable`] to
/// guarantee (at compile-time) zero-termination of device id tables provided by drivers.
///
/// # Safety
///
/// Implementers must ensure that:
///   - [`RawDeviceId::ZERO`] is actually a zeroed-out version of the raw device id.
///   - [`RawDeviceId::to_rawid`] stores `offset` in the context/data field of the raw device id so
///     that buses can recover the pointer to the data.
#[const_trait]
pub unsafe trait RawDeviceId {
    /// The raw type that holds the device id.
    ///
    /// Id tables created from [`Self`] are going to hold this type in its zero-terminated array.
    type RawType: Copy;

    /// A zeroed-out representation of the raw device id.
    ///
    /// Id tables created from [`Self`] use [`Self::ZERO`] as the sentinel to indicate the end of
    /// the table.
    const ZERO: Self::RawType;

    /// Converts an id into a raw id.
    ///
    /// `offset` is the offset from the memory location where the raw device id is stored to the
    /// location where its associated context information is stored. Implementations must store
    /// this in the appropriate context/data field of the raw type.
    fn to_rawid(&self, offset: isize) -> Self::RawType;
}

/// A zero-terminated device id array.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct IdArrayIds<T: RawDeviceId, const N: usize> {
    ids: [T::RawType; N],
    sentinel: T::RawType,
}

unsafe impl<T: RawDeviceId, const N: usize> Sync for IdArrayIds<T, N> {}

/// A zero-terminated device id array, followed by context data.
#[repr(C)]
pub struct IdArray<T: RawDeviceId, U, const N: usize> {
    ids: IdArrayIds<T, N>,
    id_infos: [Option<U>; N],
}

impl<T: RawDeviceId, U, const N: usize> IdArray<T, U, N> {
    /// Creates a new instance of the array.
    ///
    /// The contents are derived from the given identifiers and context information.
    pub const fn new(ids: [T; N], infos: [Option<U>; N]) -> Self
    where
        T: ~const RawDeviceId + Copy,
        T::RawType: Copy + Clone,
    {
        let mut array = Self {
            ids: IdArrayIds {
                ids: [T::ZERO; N],
                sentinel: T::ZERO,
            },
            id_infos: infos,
        };
        let mut i = 0usize;
        while i < N {
            // SAFETY: Both pointers are within `array` (or one byte beyond), consequently they are
            // derived from the same allocated object. We are using a `u8` pointer, whose size 1,
            // so the pointers are necessarily 1-byte aligned.
            let offset = unsafe {
                (&array.id_infos[i] as *const _ as *const u8)
                    .offset_from(&array.ids.ids[i] as *const _ as _)
            };
            array.ids.ids[i] = ids[i].to_rawid(offset);
            i += 1;
        }
        array
    }

    /// Returns an `IdTable` backed by `self`.
    ///
    /// This is used to essentially erase the array size.
    pub const fn as_table(&self) -> IdTable<'_, T, U> {
        IdTable {
            first: &self.ids.ids[0],
            _p: PhantomData,
        }
    }

    /// Returns the number of items in the ID table.
    pub const fn count(&self) -> usize {
        self.ids.ids.len()
    }

    /// Returns the inner IdArrayIds array, without the context data.
    pub const fn as_ids(&self) -> IdArrayIds<T, N>
    where
        T: ~const RawDeviceId + Copy,
    {
        self.ids
    }
}

/// A device id table.
///
/// The table is guaranteed to be zero-terminated and to be followed by an array of context data of
/// type `Option<U>`.
pub struct IdTable<'a, T: RawDeviceId, U> {
    first: &'a T::RawType,
    _p: PhantomData<&'a U>,
}

impl<T: RawDeviceId, U> const AsRef<T::RawType> for IdTable<'_, T, U> {
    fn as_ref(&self) -> &T::RawType {
        self.first
    }
}

/// Counts the number of parenthesis-delimited, comma-separated items.
///
/// # Examples
///
/// ```
/// # use kernel::count_paren_items;
///
/// assert_eq!(0, count_paren_items!());
/// assert_eq!(1, count_paren_items!((A)));
/// assert_eq!(1, count_paren_items!((A),));
/// assert_eq!(2, count_paren_items!((A), (B)));
/// assert_eq!(2, count_paren_items!((A), (B),));
/// assert_eq!(3, count_paren_items!((A), (B), (C)));
/// assert_eq!(3, count_paren_items!((A), (B), (C),));
/// ```
#[macro_export]
macro_rules! count_paren_items {
    (($($item:tt)*), $($remaining:tt)*) => { 1 + $crate::count_paren_items!($($remaining)*) };
    (($($item:tt)*)) => { 1 };
    () => { 0 };
}

/// Converts a comma-separated list of pairs into an array with the first element. That is, it
/// discards the second element of the pair.
///
/// Additionally, it automatically introduces a type if the first element is warpped in curly
/// braces, for example, if it's `{v: 10}`, it becomes `X { v: 10 }`; this is to avoid repeating
/// the type.
///
/// # Examples
///
/// ```
/// # use kernel::first_item;
///
/// #[derive(PartialEq, Debug)]
/// struct X {
///     v: u32,
/// }
///
/// assert_eq!([] as [X; 0], first_item!(X, ));
/// assert_eq!([X { v: 10 }], first_item!(X, ({ v: 10 }, Y)));
/// assert_eq!([X { v: 10 }], first_item!(X, ({ v: 10 }, Y),));
/// assert_eq!([X { v: 10 }], first_item!(X, (X { v: 10 }, Y)));
/// assert_eq!([X { v: 10 }], first_item!(X, (X { v: 10 }, Y),));
/// assert_eq!([X { v: 10 }, X { v: 20 }], first_item!(X, ({ v: 10 }, Y), ({ v: 20 }, Y)));
/// assert_eq!([X { v: 10 }, X { v: 20 }], first_item!(X, ({ v: 10 }, Y), ({ v: 20 }, Y),));
/// assert_eq!([X { v: 10 }, X { v: 20 }], first_item!(X, (X { v: 10 }, Y), (X { v: 20 }, Y)));
/// assert_eq!([X { v: 10 }, X { v: 20 }], first_item!(X, (X { v: 10 }, Y), (X { v: 20 }, Y),));
/// assert_eq!([X { v: 10 }, X { v: 20 }, X { v: 30 }],
///            first_item!(X, ({ v: 10 }, Y), ({ v: 20 }, Y), ({v: 30}, Y)));
/// assert_eq!([X { v: 10 }, X { v: 20 }, X { v: 30 }],
///            first_item!(X, ({ v: 10 }, Y), ({ v: 20 }, Y), ({v: 30}, Y),));
/// assert_eq!([X { v: 10 }, X { v: 20 }, X { v: 30 }],
///            first_item!(X, (X { v: 10 }, Y), (X { v: 20 }, Y), (X {v: 30}, Y)));
/// assert_eq!([X { v: 10 }, X { v: 20 }, X { v: 30 }],
///            first_item!(X, (X { v: 10 }, Y), (X { v: 20 }, Y), (X {v: 30}, Y),));
/// ```
#[macro_export]
macro_rules! first_item {
    ($id_type:ty, $(({$($first:tt)*}, $second:expr)),* $(,)?) => {
        {
            type IdType = $id_type;
            [$(IdType{$($first)*},)*]
        }
    };
    ($id_type:ty, $(($first:expr, $second:expr)),* $(,)?) => { [$($first,)*] };
}

/// Converts a comma-separated list of pairs into an array with the second element. That is, it
/// discards the first element of the pair.
///
/// # Examples
///
/// ```
/// # use kernel::second_item;
///
/// assert_eq!([] as [u32; 0], second_item!());
/// assert_eq!([10u32], second_item!((X, 10u32)));
/// assert_eq!([10u32], second_item!((X, 10u32),));
/// assert_eq!([10u32], second_item!(({ X }, 10u32)));
/// assert_eq!([10u32], second_item!(({ X }, 10u32),));
/// assert_eq!([10u32, 20], second_item!((X, 10u32), (X, 20)));
/// assert_eq!([10u32, 20], second_item!((X, 10u32), (X, 20),));
/// assert_eq!([10u32, 20], second_item!(({ X }, 10u32), ({ X }, 20)));
/// assert_eq!([10u32, 20], second_item!(({ X }, 10u32), ({ X }, 20),));
/// assert_eq!([10u32, 20, 30], second_item!((X, 10u32), (X, 20), (X, 30)));
/// assert_eq!([10u32, 20, 30], second_item!((X, 10u32), (X, 20), (X, 30),));
/// assert_eq!([10u32, 20, 30], second_item!(({ X }, 10u32), ({ X }, 20), ({ X }, 30)));
/// assert_eq!([10u32, 20, 30], second_item!(({ X }, 10u32), ({ X }, 20), ({ X }, 30),));
/// ```
#[macro_export]
macro_rules! second_item {
    ($(({$($first:tt)*}, $second:expr)),* $(,)?) => { [$($second,)*] };
    ($(($first:expr, $second:expr)),* $(,)?) => { [$($second,)*] };
}

/// Defines a new constant [`IdArray`] with a concise syntax.
///
/// It is meant to be used by buses and subsystems to create a similar macro with their device id
/// type already specified, i.e., with fewer parameters to the end user.
///
/// # Examples
///
// TODO: Exported but not usable by kernel modules (requires `const_trait_impl`).
/// ```ignore
/// #![feature(const_trait_impl)]
/// # use kernel::{define_id_array, driver::RawDeviceId};
///
/// #[derive(Copy, Clone)]
/// struct Id(u32);
///
/// // SAFETY: `ZERO` is all zeroes and `to_rawid` stores `offset` as the second element of the raw
/// // device id pair.
/// unsafe impl const RawDeviceId for Id {
///     type RawType = (u64, isize);
///     const ZERO: Self::RawType = (0, 0);
///     fn to_rawid(&self, offset: isize) -> Self::RawType {
///         (self.0 as u64 + 1, offset)
///     }
/// }
///
/// define_id_array!(A1, Id, (), []);
/// define_id_array!(A2, Id, &'static [u8], [(Id(10), None)]);
/// define_id_array!(A3, Id, &'static [u8], [(Id(10), Some(b"id1")), ]);
/// define_id_array!(A4, Id, &'static [u8], [(Id(10), Some(b"id1")), (Id(20), Some(b"id2"))]);
/// define_id_array!(A5, Id, &'static [u8], [(Id(10), Some(b"id1")), (Id(20), Some(b"id2")), ]);
/// define_id_array!(A6, Id, &'static [u8], [(Id(10), None), (Id(20), Some(b"id2")), ]);
/// define_id_array!(A7, Id, &'static [u8], [(Id(10), Some(b"id1")), (Id(20), None), ]);
/// define_id_array!(A8, Id, &'static [u8], [(Id(10), None), (Id(20), None), ]);
///
/// // Within a bus driver:
/// driver_id_table!(BUS_ID_TABLE, Id, &'static [u8], A1);
/// // At the top level:
/// module_id_table!(MODULE_ID_TABLE, "mybus", Id, A1);
/// ```
#[macro_export]
macro_rules! define_id_array {
    ($table_name:ident, $id_type:ty, $data_type:ty, [ $($t:tt)* ]) => {
        const $table_name:
            $crate::driver::IdArray<$id_type, $data_type, { $crate::count_paren_items!($($t)*) }> =
                $crate::driver::IdArray::new(
                    $crate::first_item!($id_type, $($t)*), $crate::second_item!($($t)*));
    };
}

/// Declares an [`IdArray`] as an [`IdTable`] for a bus driver with a concise syntax.
///
/// It is meant to be used by buses and subsystems to create a similar macro with their device id
/// type already specified, i.e., with fewer parameters to the end user.
///
/// # Examples
///
// TODO: Exported but not usable by kernel modules (requires `const_trait_impl`).
/// ```ignore
/// #![feature(const_trait_impl)]
/// # use kernel::{driver_id_table};

/// driver_id_table!(BUS_ID_TABLE, Id, &'static [u8], MY_ID_ARRAY);
/// ```
#[macro_export]
macro_rules! driver_id_table {
    ($table_name:ident, $id_type:ty, $data_type:ty, $target:expr) => {
        const $table_name: Option<$crate::driver::IdTable<'static, $id_type, $data_type>> =
            Some($target.as_table());
    };
}

/// Declares an [`IdArray`] as a module-level ID tablewith a concise syntax.
///
/// It is meant to be used by buses and subsystems to create a similar macro with their device id
/// type already specified, i.e., with fewer parameters to the end user.
///
/// # Examples
///
// TODO: Exported but not usable by kernel modules (requires `const_trait_impl`).
/// ```ignore
/// #![feature(const_trait_impl)]
/// # use kernel::{driver_id_table};

/// driver_id_table!(BUS_ID_TABLE, Id, &'static [u8], MY_ID_ARRAY);
/// ```
#[macro_export]
macro_rules! module_id_table {
    ($item_name:ident, $table_type:literal, $id_type:ty, $table_name:ident) => {
        #[export_name = concat!("__mod_", $table_type, "__", stringify!($table_name), "_device_table")]
        static $item_name: $crate::driver::IdArrayIds<$id_type, { $table_name.count() }> =
            $table_name.as_ids();
    };
}

/// Custom code within device removal.
pub trait DeviceRemoval {
    /// Cleans resources up when the device is removed.
    ///
    /// This is called when a device is removed and offers implementers the chance to run some code
    /// that cleans state up.
    fn device_remove(&self);
}

impl DeviceRemoval for () {
    fn device_remove(&self) {}
}

impl<T: DeviceRemoval> DeviceRemoval for Arc<T> {
    fn device_remove(&self) {
        self.deref().device_remove();
    }
}

impl<T: DeviceRemoval> DeviceRemoval for Box<T> {
    fn device_remove(&self) {
        self.deref().device_remove();
    }
}

/// A kernel module that only registers the given driver on init.
///
/// This is a helper struct to make it easier to define single-functionality modules, in this case,
/// modules that offer a single driver.
pub struct Module<T: DriverOps> {
    _driver: Pin<Box<Registration<T>>>,
}

impl<T: DriverOps> crate::Module for Module<T> {
    fn init(name: &'static CStr, module: &'static ThisModule) -> Result<Self> {
        Ok(Self {
            _driver: Registration::new_pinned(name, module)?,
        })
    }
}

/// Declares a kernel module that exposes a single driver.
///
/// It is meant to be used as a helper by other subsystems so they can more easily expose their own
/// macros.
#[macro_export]
macro_rules! module_driver {
    (<$gen_type:ident>, $driver_ops:ty, { type: $type:ty, $($f:tt)* }) => {
        type Ops<$gen_type> = $driver_ops;
        type ModuleType = $crate::driver::Module<Ops<$type>>;
        $crate::prelude::module! {
            type: ModuleType,
            $($f)*
        }
    }
}
