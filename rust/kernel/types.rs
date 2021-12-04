// SPDX-License-Identifier: GPL-2.0

//! Kernel types.
//!
//! C header: [`include/linux/types.h`](../../../../include/linux/types.h)

use crate::{
    bindings, c_types,
    sync::{Ref, RefBorrow},
};
use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    ops::{self, Deref, DerefMut},
    pin::Pin,
};

/// Permissions.
///
/// C header: [`include/uapi/linux/stat.h`](../../../../include/uapi/linux/stat.h)
///
/// C header: [`include/linux/stat.h`](../../../../include/linux/stat.h)
pub struct Mode(bindings::umode_t);

impl Mode {
    /// Creates a [`Mode`] from an integer.
    pub fn from_int(m: u16) -> Mode {
        Mode(m)
    }

    /// Returns the mode as an integer.
    pub fn as_int(&self) -> u16 {
        self.0
    }
}

/// Used to convert an object into a raw pointer that represents it.
///
/// It can eventually be converted back into the object. This is used to store objects as pointers
/// in kernel data structures, for example, an implementation of [`FileOperations`] in `struct
/// file::private_data`.
pub trait PointerWrapper {
    /// Type of values borrowed between calls to [`PointerWrapper::into_pointer`] and
    /// [`PointerWrapper::from_pointer`].
    type Borrowed<'a>;

    /// Returns the raw pointer.
    fn into_pointer(self) -> *const c_types::c_void;

    /// Returns a borrowed value.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by a previous call to [`PointerWrapper::into_pointer`].
    /// Additionally, [`PointerWrapper::from_pointer`] can only be called after *all* values
    /// returned by [`PointerWrapper::borrow`] have been dropped.
    unsafe fn borrow<'a>(ptr: *const c_types::c_void) -> Self::Borrowed<'a>;

    /// Returns the instance back from the raw pointer.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous call to [`PointerWrapper::into_pointer()`].
    unsafe fn from_pointer(ptr: *const c_types::c_void) -> Self;
}

impl<T: 'static> PointerWrapper for Box<T> {
    type Borrowed<'a> = &'a T;

    fn into_pointer(self) -> *const c_types::c_void {
        Box::into_raw(self) as _
    }

    unsafe fn borrow<'a>(ptr: *const c_types::c_void) -> &'a T {
        // SAFETY: The safety requirements for this function ensure that the object is still alive,
        // so it is safe to dereference the raw pointer.
        // The safety requirements also ensure that the object remains alive for the lifetime of
        // the returned value.
        unsafe { &*ptr.cast() }
    }

    unsafe fn from_pointer(ptr: *const c_types::c_void) -> Self {
        // SAFETY: The passed pointer comes from a previous call to [`Self::into_pointer()`].
        unsafe { Box::from_raw(ptr as _) }
    }
}

impl<T: 'static> PointerWrapper for Ref<T> {
    type Borrowed<'a> = RefBorrow<'a, T>;

    fn into_pointer(self) -> *const c_types::c_void {
        Ref::into_usize(self) as _
    }

    unsafe fn borrow<'a>(ptr: *const c_types::c_void) -> RefBorrow<'a, T> {
        // SAFETY: The safety requirements for this function ensure that the underlying object
        // remains valid for the lifetime of the returned value.
        unsafe { Ref::borrow_usize(ptr as _) }
    }

    unsafe fn from_pointer(ptr: *const c_types::c_void) -> Self {
        // SAFETY: The passed pointer comes from a previous call to [`Self::into_pointer()`].
        unsafe { Ref::from_usize(ptr as _) }
    }
}

impl<T: PointerWrapper + Deref> PointerWrapper for Pin<T> {
    type Borrowed<'a> = T::Borrowed<'a>;

    fn into_pointer(self) -> *const c_types::c_void {
        // SAFETY: We continue to treat the pointer as pinned by returning just a pointer to it to
        // the caller.
        let inner = unsafe { Pin::into_inner_unchecked(self) };
        inner.into_pointer()
    }

    unsafe fn borrow<'a>(ptr: *const c_types::c_void) -> Self::Borrowed<'a> {
        // SAFETY: The safety requirements for this function are the same as the ones for
        // `T::borrow`.
        unsafe { T::borrow(ptr) }
    }

    unsafe fn from_pointer(p: *const c_types::c_void) -> Self {
        // SAFETY: The object was originally pinned.
        // The passed pointer comes from a previous call to `inner::into_pointer()`.
        unsafe { Pin::new_unchecked(T::from_pointer(p)) }
    }
}

impl<T> PointerWrapper for *mut T {
    type Borrowed<'a> = *mut T;

    fn into_pointer(self) -> *const c_types::c_void {
        self as _
    }

    unsafe fn borrow<'a>(ptr: *const c_types::c_void) -> Self::Borrowed<'a> {
        ptr as _
    }

    unsafe fn from_pointer(ptr: *const c_types::c_void) -> Self {
        ptr as _
    }
}

impl PointerWrapper for () {
    type Borrowed<'a> = ();

    fn into_pointer(self) -> *const c_types::c_void {
        // We use 1 to be different from a null pointer.
        1usize as _
    }

    unsafe fn borrow<'a>(_: *const c_types::c_void) -> Self::Borrowed<'a> {}

    unsafe fn from_pointer(_: *const c_types::c_void) -> Self {}
}

/// Runs a cleanup function/closure when dropped.
///
/// The [`ScopeGuard::dismiss`] function prevents the cleanup function from running.
///
/// # Examples
///
/// In the example below, we have multiple exit paths and we want to log regardless of which one is
/// taken:
/// ```
/// # use kernel::prelude::*;
/// # use kernel::ScopeGuard;
/// fn example1(arg: bool) {
///     let _log = ScopeGuard::new(|| pr_info!("example1 completed\n"));
///
///     if arg {
///         return;
///     }
///
///     // Do something...
/// }
/// ```
///
/// In the example below, we want to log the same message on all early exits but a different one on
/// the main exit path:
/// ```
/// # use kernel::prelude::*;
/// # use kernel::ScopeGuard;
/// fn example2(arg: bool) {
///     let log = ScopeGuard::new(|| pr_info!("example2 returned early\n"));
///
///     if arg {
///         return;
///     }
///
///     // (Other early returns...)
///
///     log.dismiss();
///     pr_info!("example2 no early return\n");
/// }
/// ```
///
/// In the example below, we need a mutable object (the vector) to be accessible within the log
/// function, so we wrap it in the [`ScopeGuard`]:
/// ```
/// # use kernel::prelude::*;
/// # use kernel::ScopeGuard;
/// fn example3(arg: bool) -> Result {
///     let mut vec =
///         ScopeGuard::new_with_data(Vec::new(), |v| pr_info!("vec had {} elements\n", v.len()));
///
///     vec.try_push(10u8)?;
///     if arg {
///         return Ok(());
///     }
///     vec.try_push(20u8)?;
///     Ok(())
/// }
/// ```
///
/// # Invariants
///
/// The value stored in the struct is nearly always `Some(_)`, except between
/// [`ScopeGuard::dismiss`] and [`ScopeGuard::drop`]: in this case, it will be `None` as the value
/// will have been returned to the caller. Since  [`ScopeGuard::dismiss`] consumes the guard,
/// callers won't be able to use it anymore.
pub struct ScopeGuard<T, F: FnOnce(T)>(Option<(T, F)>);

impl<T, F: FnOnce(T)> ScopeGuard<T, F> {
    /// Creates a new guarded object wrapping the given data and with the given cleanup function.
    pub fn new_with_data(data: T, cleanup_func: F) -> Self {
        // INVARIANT: The struct is being initialised with `Some(_)`.
        Self(Some((data, cleanup_func)))
    }

    /// Prevents the cleanup function from running and returns the guarded data.
    pub fn dismiss(mut self) -> T {
        // INVARIANT: This is the exception case in the invariant; it is not visible to callers
        // because this function consumes `self`.
        self.0.take().unwrap().0
    }
}

impl ScopeGuard<(), Box<dyn FnOnce(())>> {
    /// Creates a new guarded object with the given cleanup function.
    pub fn new(cleanup: impl FnOnce()) -> ScopeGuard<(), impl FnOnce(())> {
        ScopeGuard::new_with_data((), move |_| cleanup())
    }
}

impl<T, F: FnOnce(T)> Deref for ScopeGuard<T, F> {
    type Target = T;

    fn deref(&self) -> &T {
        // The type invariants guarantee that `unwrap` will succeed.
        &self.0.as_ref().unwrap().0
    }
}

impl<T, F: FnOnce(T)> DerefMut for ScopeGuard<T, F> {
    fn deref_mut(&mut self) -> &mut T {
        // The type invariants guarantee that `unwrap` will succeed.
        &mut self.0.as_mut().unwrap().0
    }
}

impl<T, F: FnOnce(T)> Drop for ScopeGuard<T, F> {
    fn drop(&mut self) {
        // Run the cleanup function if one is still present.
        if let Some((data, cleanup)) = self.0.take() {
            cleanup(data)
        }
    }
}

/// Stores an opaque value.
///
/// This is meant to be used with FFI objects that are never interpreted by Rust code.
pub struct Opaque<T>(MaybeUninit<UnsafeCell<T>>);

impl<T> Opaque<T> {
    /// Creates a new opaque value.
    pub fn new(value: T) -> Self {
        Self(MaybeUninit::new(UnsafeCell::new(value)))
    }

    /// Creates an uninitialised value.
    pub fn uninit() -> Self {
        Self(MaybeUninit::uninit())
    }

    /// Returns a raw pointer to the opaque data.
    pub fn get(&self) -> *mut T {
        UnsafeCell::raw_get(self.0.as_ptr())
    }
}

/// A bitmask.
///
/// It has a restriction that all bits must be the same, except one. For example, `0b1110111` and
/// `0b1000` are acceptable masks.
#[derive(Clone, Copy)]
pub struct Bit<T> {
    index: T,
    inverted: bool,
}

/// Creates a bit mask with a single bit set.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::bit;
/// let mut x = 0xfeu32;
///
/// assert_eq!(x & bit(0), 0);
/// assert_eq!(x & bit(1), 2);
/// assert_eq!(x & bit(2), 4);
/// assert_eq!(x & bit(3), 8);
///
/// x |= bit(0);
/// assert_eq!(x, 0xff);
///
/// x &= !bit(1);
/// assert_eq!(x, 0xfd);
///
/// x &= !bit(7);
/// assert_eq!(x, 0x7d);
///
/// let y: u64 = bit(34).into();
/// assert_eq!(y, 0x400000000);
///
/// assert_eq!(y | bit(35), 0xc00000000);
/// ```
pub fn bit<T: Copy>(index: T) -> Bit<T> {
    Bit {
        index,
        inverted: false,
    }
}

impl<T: Copy> ops::Not for Bit<T> {
    type Output = Self;
    fn not(self) -> Self {
        Self {
            index: self.index,
            inverted: !self.inverted,
        }
    }
}

/// Implemented by integer types that allow counting the number of trailing zeroes.
pub trait TrailingZeros {
    /// Returns the number of trailing zeroes in the binary representation of `self`.
    fn trailing_zeros(&self) -> u32;
}

macro_rules! define_unsigned_number_traits {
    ($type_name:ty) => {
        impl TrailingZeros for $type_name {
            fn trailing_zeros(&self) -> u32 {
                <$type_name>::trailing_zeros(*self)
            }
        }

        impl<T: Copy> core::convert::From<Bit<T>> for $type_name
        where
            Self: ops::Shl<T, Output = Self> + core::convert::From<u8> + ops::Not<Output = Self>,
        {
            fn from(v: Bit<T>) -> Self {
                let c = Self::from(1u8) << v.index;
                if v.inverted {
                    !c
                } else {
                    c
                }
            }
        }

        impl<T: Copy> ops::BitAnd<Bit<T>> for $type_name
        where
            Self: ops::Shl<T, Output = Self> + core::convert::From<u8>,
        {
            type Output = Self;
            fn bitand(self, rhs: Bit<T>) -> Self::Output {
                self & Self::from(rhs)
            }
        }

        impl<T: Copy> ops::BitOr<Bit<T>> for $type_name
        where
            Self: ops::Shl<T, Output = Self> + core::convert::From<u8>,
        {
            type Output = Self;
            fn bitor(self, rhs: Bit<T>) -> Self::Output {
                self | Self::from(rhs)
            }
        }

        impl<T: Copy> ops::BitAndAssign<Bit<T>> for $type_name
        where
            Self: ops::Shl<T, Output = Self> + core::convert::From<u8>,
        {
            fn bitand_assign(&mut self, rhs: Bit<T>) {
                *self &= Self::from(rhs)
            }
        }

        impl<T: Copy> ops::BitOrAssign<Bit<T>> for $type_name
        where
            Self: ops::Shl<T, Output = Self> + core::convert::From<u8>,
        {
            fn bitor_assign(&mut self, rhs: Bit<T>) {
                *self |= Self::from(rhs)
            }
        }
    };
}

define_unsigned_number_traits!(u8);
define_unsigned_number_traits!(u16);
define_unsigned_number_traits!(u32);
define_unsigned_number_traits!(u64);
define_unsigned_number_traits!(usize);

/// Returns an iterator over the set bits of `value`.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::bits_iter;
///
/// let mut iter = bits_iter(5usize);
/// assert_eq!(iter.next().unwrap(), 0);
/// assert_eq!(iter.next().unwrap(), 2);
/// assert!(iter.next().is_none());
/// ```
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::bits_iter;
///
/// fn print_bits(x: usize) {
///     for bit in bits_iter(x) {
///         println!("{}", bit);
///     }
/// }
/// ```
#[inline]
pub fn bits_iter<T>(value: T) -> impl Iterator<Item = u32>
where
    T: core::cmp::PartialEq
        + From<u8>
        + ops::Shl<u32, Output = T>
        + ops::Not<Output = T>
        + ops::BitAndAssign
        + TrailingZeros,
{
    struct BitIterator<U> {
        value: U,
    }

    impl<U> Iterator for BitIterator<U>
    where
        U: core::cmp::PartialEq
            + From<u8>
            + ops::Shl<u32, Output = U>
            + ops::Not<Output = U>
            + ops::BitAndAssign
            + TrailingZeros,
    {
        type Item = u32;

        #[inline]
        fn next(&mut self) -> Option<u32> {
            if self.value == U::from(0u8) {
                return None;
            }
            let ret = self.value.trailing_zeros();
            self.value &= !(U::from(1u8) << ret);
            Some(ret)
        }
    }

    BitIterator { value }
}
