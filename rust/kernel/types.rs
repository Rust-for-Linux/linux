// SPDX-License-Identifier: GPL-2.0

//! Kernel types.
//!
//! C header: [`include/linux/types.h`](../../../../include/linux/types.h)

use crate::{
    bindings,
    sync::{Arc, ArcBorrow},
};
use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{self, Deref, DerefMut},
    pin::Pin,
    ptr::{addr_of, NonNull},
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
/// in kernel data structures, for example, an implementation of
/// [`Operations`][crate::file::Operations] in `struct
/// file::private_data`.
pub trait PointerWrapper {
    /// Type of values borrowed between calls to [`PointerWrapper::into_pointer`] and
    /// [`PointerWrapper::from_pointer`].
    type Borrowed<'a>;

    /// Returns the raw pointer.
    fn into_pointer(self) -> *const core::ffi::c_void;

    /// Returns a borrowed value.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by a previous call to [`PointerWrapper::into_pointer`].
    /// Additionally, [`PointerWrapper::from_pointer`] can only be called after *all* values
    /// returned by [`PointerWrapper::borrow`] have been dropped.
    unsafe fn borrow<'a>(ptr: *const core::ffi::c_void) -> Self::Borrowed<'a>;

    /// Returns a mutably borrowed value.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous to [`PointerWrapper::into_pointer`], and no
    /// other concurrent users of the pointer (except the ones derived from the returned value) run
    /// at least until the returned [`ScopeGuard`] is dropped.
    unsafe fn borrow_mut<T: PointerWrapper>(ptr: *const core::ffi::c_void) -> ScopeGuard<T, fn(T)> {
        // SAFETY: The safety requirements ensure that `ptr` came from a previous call to
        // `into_pointer`.
        ScopeGuard::new_with_data(unsafe { T::from_pointer(ptr) }, |d| {
            d.into_pointer();
        })
    }

    /// Returns the instance back from the raw pointer.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous call to [`PointerWrapper::into_pointer()`].
    unsafe fn from_pointer(ptr: *const core::ffi::c_void) -> Self;
}

impl<T: 'static> PointerWrapper for Box<T> {
    type Borrowed<'a> = &'a T;

    fn into_pointer(self) -> *const core::ffi::c_void {
        Box::into_raw(self) as _
    }

    unsafe fn borrow<'a>(ptr: *const core::ffi::c_void) -> &'a T {
        // SAFETY: The safety requirements for this function ensure that the object is still alive,
        // so it is safe to dereference the raw pointer.
        // The safety requirements also ensure that the object remains alive for the lifetime of
        // the returned value.
        unsafe { &*ptr.cast() }
    }

    unsafe fn from_pointer(ptr: *const core::ffi::c_void) -> Self {
        // SAFETY: The passed pointer comes from a previous call to [`Self::into_pointer()`].
        unsafe { Box::from_raw(ptr as _) }
    }
}

impl<T: 'static> PointerWrapper for Arc<T> {
    type Borrowed<'a> = ArcBorrow<'a, T>;

    fn into_pointer(self) -> *const core::ffi::c_void {
        Arc::into_usize(self) as _
    }

    unsafe fn borrow<'a>(ptr: *const core::ffi::c_void) -> ArcBorrow<'a, T> {
        // SAFETY: The safety requirements for this function ensure that the underlying object
        // remains valid for the lifetime of the returned value.
        unsafe { Arc::borrow_usize(ptr as _) }
    }

    unsafe fn from_pointer(ptr: *const core::ffi::c_void) -> Self {
        // SAFETY: The passed pointer comes from a previous call to [`Self::into_pointer()`].
        unsafe { Arc::from_usize(ptr as _) }
    }
}

impl<T: PointerWrapper + Deref> PointerWrapper for Pin<T> {
    type Borrowed<'a> = T::Borrowed<'a>;

    fn into_pointer(self) -> *const core::ffi::c_void {
        // SAFETY: We continue to treat the pointer as pinned by returning just a pointer to it to
        // the caller.
        let inner = unsafe { Pin::into_inner_unchecked(self) };
        inner.into_pointer()
    }

    unsafe fn borrow<'a>(ptr: *const core::ffi::c_void) -> Self::Borrowed<'a> {
        // SAFETY: The safety requirements for this function are the same as the ones for
        // `T::borrow`.
        unsafe { T::borrow(ptr) }
    }

    unsafe fn from_pointer(p: *const core::ffi::c_void) -> Self {
        // SAFETY: The object was originally pinned.
        // The passed pointer comes from a previous call to `inner::into_pointer()`.
        unsafe { Pin::new_unchecked(T::from_pointer(p)) }
    }
}

impl<T> PointerWrapper for *mut T {
    type Borrowed<'a> = *mut T;

    fn into_pointer(self) -> *const core::ffi::c_void {
        self as _
    }

    unsafe fn borrow<'a>(ptr: *const core::ffi::c_void) -> Self::Borrowed<'a> {
        ptr as _
    }

    unsafe fn from_pointer(ptr: *const core::ffi::c_void) -> Self {
        ptr as _
    }
}

impl PointerWrapper for () {
    type Borrowed<'a> = ();

    fn into_pointer(self) -> *const core::ffi::c_void {
        // We use 1 to be different from a null pointer.
        1usize as _
    }

    unsafe fn borrow<'a>(_: *const core::ffi::c_void) -> Self::Borrowed<'a> {}

    unsafe fn from_pointer(_: *const core::ffi::c_void) -> Self {}
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
/// # use kernel::ScopeGuard;
/// fn example1(arg: bool) {
///     let _log = ScopeGuard::new(|| pr_info!("example1 completed\n"));
///
///     if arg {
///         return;
///     }
///
///     pr_info!("Do something...\n");
/// }
///
/// # example1(false);
/// # example1(true);
/// ```
///
/// In the example below, we want to log the same message on all early exits but a different one on
/// the main exit path:
/// ```
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
///
/// # example2(false);
/// # example2(true);
/// ```
///
/// In the example below, we need a mutable object (the vector) to be accessible within the log
/// function, so we wrap it in the [`ScopeGuard`]:
/// ```
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
///
/// # assert_eq!(example3(false), Ok(()));
/// # assert_eq!(example3(true), Ok(()));
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
#[repr(transparent)]
pub struct Opaque<T>(MaybeUninit<UnsafeCell<T>>);

impl<T> Opaque<T> {
    /// Creates a new opaque value.
    pub const fn new(value: T) -> Self {
        Self(MaybeUninit::new(UnsafeCell::new(value)))
    }

    /// Creates an uninitialised value.
    pub const fn uninit() -> Self {
        Self(MaybeUninit::uninit())
    }

    /// Returns a raw pointer to the opaque data.
    pub fn get(&self) -> *mut T {
        UnsafeCell::raw_get(self.0.as_ptr())
    }

    /// Gets the value behind `this`.
    ///
    /// This function is useful to get access to the value without creating intermeditate
    /// references.
    ///
    /// # Safety
    ///
    /// The pointer supplied is valid.
    pub unsafe fn raw_get(this: *const Self) -> *mut T {
        UnsafeCell::raw_get(addr_of!((*this).0).cast::<UnsafeCell<T>>())
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
pub const fn bit<T: Copy>(index: T) -> Bit<T> {
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
/// use kernel::bits_iter;
///
/// let mut iter = bits_iter(5usize);
/// assert_eq!(iter.next().unwrap(), 0);
/// assert_eq!(iter.next().unwrap(), 2);
/// assert!(iter.next().is_none());
/// ```
///
/// ```
/// use kernel::bits_iter;
///
/// fn print_bits(x: usize) {
///     for bit in bits_iter(x) {
///         pr_info!("{}\n", bit);
///     }
/// }
///
/// # print_bits(42);
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

/// A trait for boolean types.
///
/// This is meant to be used in type states to allow boolean constraints in implementation blocks.
/// In the example below, the implementation containing `MyType::set_value` could _not_ be
/// constrained to type states containing `Writable = true` if `Writable` were a constant instead
/// of a type.
///
/// # Safety
///
/// No additional implementations of [`Bool`] should be provided, as [`True`] and [`False`] are
/// already provided.
///
/// # Examples
///
/// ```
/// # use kernel::{Bool, False, True};
/// use core::marker::PhantomData;
///
/// // Type state specifies whether the type is writable.
/// trait MyTypeState {
///     type Writable: Bool;
/// }
///
/// // In state S1, the type is writable.
/// struct S1;
/// impl MyTypeState for S1 {
///     type Writable = True;
/// }
///
/// // In state S2, the type is not writable.
/// struct S2;
/// impl MyTypeState for S2 {
///     type Writable = False;
/// }
///
/// struct MyType<T: MyTypeState> {
///     value: u32,
///     _p: PhantomData<T>,
/// }
///
/// impl<T: MyTypeState> MyType<T> {
///     fn new(value: u32) -> Self {
///         Self {
///             value,
///             _p: PhantomData,
///         }
///     }
/// }
///
/// // This implementation block only applies if the type state is writable.
/// impl<T> MyType<T>
/// where
///     T: MyTypeState<Writable = True>,
/// {
///     fn set_value(&mut self, v: u32) {
///         self.value = v;
///     }
/// }
///
/// let mut x = MyType::<S1>::new(10);
/// let mut y = MyType::<S2>::new(20);
///
/// x.set_value(30);
///
/// // The code below fails to compile because `S2` is not writable.
/// // y.set_value(40);
/// ```
pub unsafe trait Bool {}

/// Represents the `true` value for types with [`Bool`] bound.
pub struct True;

// SAFETY: This is one of the only two implementations of `Bool`.
unsafe impl Bool for True {}

/// Represents the `false` value for types wth [`Bool`] bound.
pub struct False;

// SAFETY: This is one of the only two implementations of `Bool`.
unsafe impl Bool for False {}

/// Types that are _always_ reference counted.
///
/// It allows such types to define their own custom ref increment and decrement functions.
/// Additionally, it allows users to convert from a shared reference `&T` to an owned reference
/// [`ARef<T>`].
///
/// This is usually implemented by wrappers to existing structures on the C side of the code. For
/// Rust code, the recommendation is to use [`Arc`] to create reference-counted instances of a
/// type.
///
/// # Safety
///
/// Implementers must ensure that increments to the reference count keeps the object alive in
/// memory at least until a matching decrement performed.
///
/// Implementers must also ensure that all instances are reference-counted. (Otherwise they
/// won't be able to honour the requirement that [`AlwaysRefCounted::inc_ref`] keep the object
/// alive.)
pub unsafe trait AlwaysRefCounted {
    /// Increments the reference count on the object.
    fn inc_ref(&self);

    /// Decrements the reference count on the object.
    ///
    /// Frees the object when the count reaches zero.
    ///
    /// # Safety
    ///
    /// Callers must ensure that there was a previous matching increment to the reference count,
    /// and that the object is no longer used after its reference count is decremented (as it may
    /// result in the object being freed), unless the caller owns another increment on the refcount
    /// (e.g., it calls [`AlwaysRefCounted::inc_ref`] twice, then calls
    /// [`AlwaysRefCounted::dec_ref`] once).
    unsafe fn dec_ref(obj: NonNull<Self>);
}

/// An owned reference to an always-reference-counted object.
///
/// The object's reference count is automatically decremented when an instance of [`ARef`] is
/// dropped. It is also automatically incremented when a new instance is created via
/// [`ARef::clone`].
///
/// # Invariants
///
/// The pointer stored in `ptr` is non-null and valid for the lifetime of the [`ARef`] instance. In
/// particular, the [`ARef`] instance owns an increment on underlying object's reference count.
pub struct ARef<T: AlwaysRefCounted> {
    ptr: NonNull<T>,
    _p: PhantomData<T>,
}

impl<T: AlwaysRefCounted> ARef<T> {
    /// Creates a new instance of [`ARef`].
    ///
    /// It takes over an increment of the reference count on the underlying object.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the reference count was incremented at least once, and that they
    /// are properly relinquishing one increment. That is, if there is only one increment, callers
    /// must not use the underlying object anymore -- it is only safe to do so via the newly
    /// created [`ARef`].
    pub unsafe fn from_raw(ptr: NonNull<T>) -> Self {
        // INVARIANT: The safety requirements guarantee that the new instance now owns the
        // increment on the refcount.
        Self {
            ptr,
            _p: PhantomData,
        }
    }
}

impl<T: AlwaysRefCounted> Clone for ARef<T> {
    fn clone(&self) -> Self {
        self.inc_ref();
        // SAFETY: We just incremented the refcount above.
        unsafe { Self::from_raw(self.ptr) }
    }
}

impl<T: AlwaysRefCounted> Deref for ARef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The type invariants guarantee that the object is valid.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: AlwaysRefCounted> From<&T> for ARef<T> {
    fn from(b: &T) -> Self {
        b.inc_ref();
        // SAFETY: We just incremented the refcount above.
        unsafe { Self::from_raw(NonNull::from(b)) }
    }
}

impl<T: AlwaysRefCounted> Drop for ARef<T> {
    fn drop(&mut self) {
        // SAFETY: The type invariants guarantee that the `ARef` owns the reference we're about to
        // decrement.
        unsafe { T::dec_ref(self.ptr) };
    }
}

/// A sum type that always holds either a value of type `L` or `R`.
pub enum Either<L, R> {
    /// Constructs an instance of [`Either`] containing a value of type `L`.
    Left(L),

    /// Constructs an instance of [`Either`] containing a value of type `R`.
    Right(R),
}
