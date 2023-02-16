// SPDX-License-Identifier: GPL-2.0

//! Devicetree and Open Firmware abstractions.
//!
//! C header: [`include/linux/of_*.h`](../../../../include/linux/of_*.h)

// Note: Most OF functions turn into inline dummies with CONFIG_OF(_*) disabled.
// We have to either add config conditionals to helpers.c or here; let's do it
// here for now. In the future, once bindgen can auto-generate static inline
// helpers, this can go away if desired.

use core::marker::PhantomData;
use core::num::NonZeroU32;

use crate::{
    bindings, driver,
    prelude::*,
    str::{BStr, CStr},
};

/// An open firmware device id.
#[derive(Clone, Copy)]
pub enum DeviceId {
    /// An open firmware device id where only a compatible string is specified.
    Compatible(&'static BStr),
}

/// Defines a const open firmware device id table that also carries per-entry data/context/info.
///
/// # Example
///
/// ```
/// # use kernel::{define_of_id_table, module_of_id_table, driver_of_id_table};
/// use kernel::of;
///
/// define_of_id_table! {MY_ID_TABLE, u32, [
///     (of::DeviceId::Compatible(b"test-device1,test-device2"), Some(0xff)),
///     (of::DeviceId::Compatible(b"test-device3"), None),
/// ]};
///
/// module_of_id_table!(MOD_TABLE, ASAHI_ID_TABLE);
///
/// // Within the `Driver` implementation:
/// driver_of_id_table!(MY_ID_TABLE);
/// ```
#[macro_export]
macro_rules! define_of_id_table {
    ($name:ident, $data_type:ty, $($t:tt)*) => {
        $crate::define_id_array!($name, $crate::of::DeviceId, $data_type, $($t)*);
    };
}

/// Convenience macro to declare which device ID table to use for a bus driver.
#[macro_export]
macro_rules! driver_of_id_table {
    ($name:expr) => {
        $crate::driver_id_table!(
            OF_DEVICE_ID_TABLE,
            $crate::of::DeviceId,
            Self::IdInfo,
            $name
        );
    };
}

/// Declare a device ID table as a module-level table. This creates the necessary module alias
/// entries to enable module autoloading.
#[macro_export]
macro_rules! module_of_id_table {
    ($item_name:ident, $table_name:ident) => {
        $crate::module_id_table!($item_name, "of", $crate::of::DeviceId, $table_name);
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
            id.compatible[i] = compatible[i] as _;
            i += 1;
        }
        id.compatible[i] = b'\0' as _;
        id.data = offset as _;
        id
    }
}

/// Type alias for an OF phandle
pub type PHandle = bindings::phandle;

/// An OF device tree node.
///
/// # Invariants
///
/// `raw_node` points to a valid OF node, and we hold a reference to it.
pub struct Node {
    raw_node: *mut bindings::device_node,
}

#[allow(dead_code)]
impl Node {
    /// Creates a `Node` from a raw C pointer. The pointer must be owned (the caller
    /// gives up its reference). If the pointer is NULL, returns None.
    pub(crate) unsafe fn from_raw(raw_node: *mut bindings::device_node) -> Option<Node> {
        if raw_node.is_null() {
            None
        } else {
            // INVARIANT: `raw_node` is valid per the above contract, and non-null per the
            // above check.
            Some(Node { raw_node })
        }
    }

    /// Creates a `Node` from a raw C pointer. The pointer must be borrowed (the caller
    /// retains its reference, which must be valid for the duration of the call). If the
    /// pointer is NULL, returns None.
    pub(crate) unsafe fn get_from_raw(raw_node: *mut bindings::device_node) -> Option<Node> {
        // SAFETY: `raw_node` is valid or NULL per the above contract. `of_node_get` can handle
        // NULL.
        unsafe {
            #[cfg(CONFIG_OF_DYNAMIC)]
            bindings::of_node_get(raw_node);
            Node::from_raw(raw_node)
        }
    }

    /// Returns a reference to the underlying C `device_node` structure.
    fn node(&self) -> &bindings::device_node {
        // SAFETY: `raw_node` is valid per the type invariant.
        unsafe { &*self.raw_node }
    }

    /// Returns the name of the node.
    pub fn name(&self) -> &CStr {
        // SAFETY: The lifetime of the `CStr` is the same as the lifetime of this `Node`.
        unsafe { CStr::from_char_ptr(self.node().name) }
    }

    /// Returns the phandle for this node.
    pub fn phandle(&self) -> PHandle {
        self.node().phandle
    }

    /// Returns the full name (with address) for this node.
    pub fn full_name(&self) -> &CStr {
        // SAFETY: The lifetime of the `CStr` is the same as the lifetime of this `Node`.
        unsafe { CStr::from_char_ptr(self.node().full_name) }
    }

    /// Returns `true` if the node is the root node.
    pub fn is_root(&self) -> bool {
        unsafe { bindings::of_node_is_root(self.raw_node) }
    }

    /// Returns the parent node, if any.
    pub fn parent(&self) -> Option<Node> {
        #[cfg(not(CONFIG_OF))]
        {
            None
        }
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant, and `of_get_parent()` takes a
        // new reference to the parent (or returns NULL).
        unsafe {
            Node::from_raw(bindings::of_get_parent(self.raw_node))
        }
    }

    /// Returns an iterator over the node's children.
    // TODO: use type alias for return type once type_alias_impl_trait is stable
    pub fn children(
        &self,
    ) -> NodeIterator<'_, impl Fn(*mut bindings::device_node) -> *mut bindings::device_node + '_>
    {
        #[cfg(not(CONFIG_OF))]
        {
            NodeIterator::new(|_prev| core::ptr::null_mut())
        }
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant, and the lifetime of the `NodeIterator`
        // does not exceed the lifetime of the `Node` so it can borrow its reference.
        NodeIterator::new(|prev| unsafe { bindings::of_get_next_child(self.raw_node, prev) })
    }

    /// Find a child by its name and return it, or None if not found.
    #[allow(unused_variables)]
    pub fn get_child_by_name(&self, name: &CStr) -> Option<Node> {
        #[cfg(not(CONFIG_OF))]
        {
            None
        }
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant.
        unsafe {
            Node::from_raw(bindings::of_get_child_by_name(
                self.raw_node,
                name.as_char_ptr(),
            ))
        }
    }

    /// Checks whether the node is compatible with the given compatible string.
    ///
    /// Returns `None` if there is no match, or `Some<NonZeroU32>` if there is, with the value
    /// representing as match score (higher values for more specific compatible matches).
    #[allow(unused_variables)]
    pub fn is_compatible(&self, compatible: &CStr) -> Option<NonZeroU32> {
        #[cfg(not(CONFIG_OF))]
        let ret = 0;
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant.
        let ret =
            unsafe { bindings::of_device_is_compatible(self.raw_node, compatible.as_char_ptr()) };

        NonZeroU32::new(ret.try_into().ok()?)
    }

    /// Parse a phandle property and return the Node referenced at a given index, if any.
    ///
    /// Used only for phandle properties with no arguments.
    #[allow(unused_variables)]
    pub fn parse_phandle(&self, name: &CStr, index: usize) -> Option<Node> {
        #[cfg(not(CONFIG_OF))]
        {
            None
        }
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant. `of_parse_phandle` returns an
        // owned reference.
        unsafe {
            Node::from_raw(bindings::of_parse_phandle(
                self.raw_node,
                name.as_char_ptr(),
                index.try_into().ok()?,
            ))
        }
    }

    #[allow(unused_variables)]
    /// Look up a node property by name, returning a `Property` object if found.
    pub fn find_property(&self, propname: &CStr) -> Option<Property<'_>> {
        #[cfg(not(CONFIG_OF))]
        {
            None
        }
        #[cfg(CONFIG_OF)]
        // SAFETY: `raw_node` is valid per the type invariant. The property structure
        // returned borrows the reference to the owning node, and so has the same
        // lifetime.
        unsafe {
            Property::from_raw(bindings::of_find_property(
                self.raw_node,
                propname.as_char_ptr(),
                core::ptr::null_mut(),
            ))
        }
    }

    /// Look up a mandatory node property by name, and decode it into a value type.
    ///
    /// Returns `Err(ENOENT)` if the property is not found.
    ///
    /// The type `T` must implement `TryFrom<Property<'_>>`.
    pub fn get_property<'a, T: TryFrom<Property<'a>>>(&'a self, propname: &CStr) -> Result<T>
    where
        crate::error::Error: From<<T as TryFrom<Property<'a>>>::Error>,
    {
        Ok(self.find_property(propname).ok_or(ENOENT)?.try_into()?)
    }

    /// Look up an optional node property by name, and decode it into a value type.
    ///
    /// Returns `Ok(None)` if the property is not found.
    ///
    /// The type `T` must implement `TryFrom<Property<'_>>`.
    pub fn get_opt_property<'a, T: TryFrom<Property<'a>>>(
        &'a self,
        propname: &CStr,
    ) -> Result<Option<T>>
    where
        crate::error::Error: From<<T as TryFrom<Property<'a>>>::Error>,
    {
        self.find_property(propname)
            .map_or(Ok(None), |p| Ok(Some(p.try_into()?)))
    }
}

/// A property attached to a device tree `Node`.
///
/// # Invariants
///
/// `raw` must be valid and point to a property that outlives the lifetime of this object.
#[derive(Copy, Clone)]
pub struct Property<'a> {
    raw: *mut bindings::property,
    _p: PhantomData<&'a Node>,
}

impl<'a> Property<'a> {
    #[cfg(CONFIG_OF)]
    /// Create a `Property` object from a raw C pointer. Returns `None` if NULL.
    ///
    /// The passed pointer must be valid and outlive the lifetime argument, or NULL.
    unsafe fn from_raw(raw: *mut bindings::property) -> Option<Property<'a>> {
        if raw.is_null() {
            None
        } else {
            Some(Property {
                raw,
                _p: PhantomData,
            })
        }
    }

    /// Returns the name of the property as a `CStr`.
    pub fn name(&self) -> &CStr {
        // SAFETY: `raw` is valid per the type invariant, and the lifetime of the `CStr` does not
        // outlive it.
        unsafe { CStr::from_char_ptr((*self.raw).name) }
    }

    /// Returns the name of the property as a `&[u8]`.
    pub fn value(&self) -> &[u8] {
        // SAFETY: `raw` is valid per the type invariant, and the lifetime of the slice does not
        // outlive it.
        unsafe { core::slice::from_raw_parts((*self.raw).value as *const u8, self.len()) }
    }

    /// Returns the length of the property in bytes.
    pub fn len(&self) -> usize {
        // SAFETY: `raw` is valid per the type invariant.
        unsafe { (*self.raw).length.try_into().unwrap() }
    }

    /// Returns true if the property is empty (zero-length), which typically represents boolean true.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A trait that represents a value decodable from a property with a fixed unit size.
///
/// This allows us to auto-derive property decode implementations for `Vec<T: PropertyUnit>`.
pub trait PropertyUnit: Sized {
    /// The size in bytes of a single data unit.
    const UNIT_SIZE: usize;

    /// Decode this data unit from a byte slice. The passed slice will have a length of `UNIT_SIZE`.
    fn from_bytes(data: &[u8]) -> Result<Self>;
}

// This doesn't work...
// impl<'a, T: PropertyUnit> TryFrom<Property<'a>> for T {
//     type Error = Error;
//
//     fn try_from(p: Property<'_>) -> core::result::Result<T, Self::Error> {
//         if p.value().len() != T::UNIT_SIZE {
//             Err(EINVAL)
//         } else {
//             Ok(T::from_bytes(p.value())?)
//         }
//     }
// }

impl<'a, T: PropertyUnit> TryFrom<Property<'a>> for Vec<T> {
    type Error = Error;

    fn try_from(p: Property<'_>) -> core::result::Result<Vec<T>, Self::Error> {
        if p.len() % T::UNIT_SIZE != 0 {
            return Err(EINVAL);
        }

        let mut v = Vec::new();
        let val = p.value();
        for off in (0..p.len()).step_by(T::UNIT_SIZE) {
            v.try_push(T::from_bytes(&val[off..off + T::UNIT_SIZE])?)?;
        }
        Ok(v)
    }
}

macro_rules! prop_int_type (
    ($type:ty) => {
        impl<'a> TryFrom<Property<'a>> for $type {
            type Error = Error;

            fn try_from(p: Property<'_>) -> core::result::Result<$type, Self::Error> {
                Ok(<$type>::from_be_bytes(p.value().try_into().or(Err(EINVAL))?))
            }
        }

        impl PropertyUnit for $type {
            const UNIT_SIZE: usize = <$type>::BITS as usize / 8;

            fn from_bytes(data: &[u8]) -> Result<Self> {
                Ok(<$type>::from_be_bytes(data.try_into().or(Err(EINVAL))?))
            }
        }
    }
);

prop_int_type!(u8);
prop_int_type!(u16);
prop_int_type!(u32);
prop_int_type!(u64);
prop_int_type!(i8);
prop_int_type!(i16);
prop_int_type!(i32);
prop_int_type!(i64);

/// An iterator across a collection of Node objects.
///
/// # Invariants
///
/// `cur` must be NULL or a valid node owned reference. If NULL, it represents either the first
/// or last position of the iterator.
///
/// If `done` is true, `cur` must be NULL.
///
/// fn_next must be a callback that iterates from one node to the next, and it must not capture
/// values that exceed the lifetime of the iterator. It must return owned references and also
/// take owned references.
pub struct NodeIterator<'a, T>
where
    T: Fn(*mut bindings::device_node) -> *mut bindings::device_node,
{
    cur: *mut bindings::device_node,
    done: bool,
    fn_next: T,
    _p: PhantomData<&'a T>,
}

impl<'a, T> NodeIterator<'a, T>
where
    T: Fn(*mut bindings::device_node) -> *mut bindings::device_node,
{
    fn new(next: T) -> NodeIterator<'a, T> {
        // INVARIANT: `cur` is initialized to NULL to represent the initial state.
        NodeIterator {
            cur: core::ptr::null_mut(),
            done: false,
            fn_next: next,
            _p: PhantomData,
        }
    }
}

impl<'a, T> Iterator for NodeIterator<'a, T>
where
    T: Fn(*mut bindings::device_node) -> *mut bindings::device_node,
{
    type Item = Node;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            None
        } else {
            // INVARIANT: if the new `cur` is NULL, then the iterator has reached its end and we
            // set `done` to `true`.
            self.cur = (self.fn_next)(self.cur);
            self.done = self.cur.is_null();
            // SAFETY: `fn_next` must return an owned reference per the iterator contract.
            // The iterator itself is considered to own this reference, so we take another one.
            unsafe { Node::get_from_raw(self.cur) }
        }
    }
}

// Drop impl to ensure we drop the current node being iterated on, if any.
impl<'a, T> Drop for NodeIterator<'a, T>
where
    T: Fn(*mut bindings::device_node) -> *mut bindings::device_node,
{
    fn drop(&mut self) {
        // SAFETY: `cur` is valid or NULL, and `of_node_put()` can handle NULL.
        #[cfg(CONFIG_OF_DYNAMIC)]
        unsafe {
            bindings::of_node_put(self.cur)
        };
    }
}

/// Returns the root node of the OF device tree (if any).
pub fn root() -> Option<Node> {
    unsafe { Node::get_from_raw(bindings::of_root) }
}

/// Returns the /chosen node of the OF device tree (if any).
pub fn chosen() -> Option<Node> {
    unsafe { Node::get_from_raw(bindings::of_chosen) }
}

/// Returns the /aliases node of the OF device tree (if any).
pub fn aliases() -> Option<Node> {
    unsafe { Node::get_from_raw(bindings::of_aliases) }
}

/// Returns the system stdout node of the OF device tree (if any).
pub fn stdout() -> Option<Node> {
    unsafe { Node::get_from_raw(bindings::of_stdout) }
}

#[allow(unused_variables)]
/// Looks up a node in the device tree by phandle.
pub fn find_node_by_phandle(handle: PHandle) -> Option<Node> {
    #[cfg(not(CONFIG_OF))]
    {
        None
    }
    #[cfg(CONFIG_OF)]
    unsafe {
        #[allow(dead_code)]
        Node::from_raw(bindings::of_find_node_by_phandle(handle))
    }
}

impl Clone for Node {
    fn clone(&self) -> Node {
        // SAFETY: `raw_node` is valid and non-NULL per the type invariant,
        // so this can never return None.
        unsafe { Node::get_from_raw(self.raw_node).unwrap() }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        #[cfg(CONFIG_OF_DYNAMIC)]
        // SAFETY: `raw_node` is valid per the type invariant.
        unsafe {
            bindings::of_node_put(self.raw_node)
        };
    }
}
