// SPDX-License-Identifier: GPL-2.0

//! String representations.

use core::ops::{self, Deref, Index};

use crate::bindings;
use crate::build_assert;
use crate::c_types;

/// Byte string without UTF-8 validity guarantee.
///
/// `BStr` is simply an alias to `[u8]`, but has a more evident semantical meaning.
pub type BStr = [u8];

/// Creates a new [`BStr`] from a string literal.
///
/// `b_str!` converts the supplied string literal to byte string, so non-ASCII
/// characters can be included.
///
/// # Examples
///
/// ```
/// # use kernel::b_str;
/// # use kernel::str::BStr;
/// const MY_BSTR: &'static BStr = b_str!("My awesome BStr!");
/// ```
#[macro_export]
macro_rules! b_str {
    ($str:literal) => {{
        const S: &'static str = $str;
        const C: &'static $crate::str::BStr = S.as_bytes();
        C
    }};
}

/// Possible errors when using conversion functions in [`CStr`] and [`CBoundedStr`].
#[derive(Debug, Clone, Copy)]
pub enum CStrConvertError {
    /// Supplied string length exceeds the specified bound. Only happens when
    /// constructing a [`CBoundedStr`].
    BoundExceeded,

    /// Supplied bytes contain an interior `NUL`.
    InteriorNul,

    /// Supplied bytes are not terminated by `NUL`.
    NotNulTerminated,
}

impl From<CStrConvertError> for crate::Error {
    #[inline]
    fn from(_: CStrConvertError) -> crate::Error {
        crate::Error::EINVAL
    }
}

/// A string that is guaranteed to have exactly one `NUL` byte, which is at the
/// end.
///
/// Used for interoperability with kernel APIs that take C strings.
#[repr(transparent)]
pub struct CStr([u8]);

impl CStr {
    /// Returns the length of this string excluding `NUL`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len_with_nul() - 1
    }

    /// Returns the length of this string with `NUL`.
    #[inline]
    pub const fn len_with_nul(&self) -> usize {
        // SAFETY: This is one of the invariant of `CStr`.
        // We add a `unreachable_unchecked` here to hint the optimizer that
        // the value returned from this function is non-zero.
        if self.0.is_empty() {
            unsafe { core::hint::unreachable_unchecked() };
        }
        self.0.len()
    }

    /// Returns `true` if the string only includes `NUL`.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Wraps a raw C string pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid pointer to a `NUL`-terminated C string, and it must
    /// last at least `'a`. When `CStr` is alive, the memory pointed by `ptr`
    /// must not be mutated.
    #[inline]
    pub unsafe fn from_char_ptr<'a>(ptr: *const c_types::c_char) -> &'a Self {
        // SAFETY: The safety precondition guarantees `ptr` is a valid pointer
        // to a `NUL`-terminated C string.
        let len = unsafe { bindings::strlen(ptr) } + 1;
        // SAFETY: Lifetime guaranteed by the safety precondition.
        let bytes = unsafe { core::slice::from_raw_parts(ptr as _, len as _) };
        // SAFETY: As `len` is returned by `strlen`, `bytes` does not contain interior `NUL`.
        // As we have added 1 to `len`, the last byte is known to be `NUL`.
        unsafe { Self::from_bytes_with_nul_unchecked(bytes) }
    }

    /// Creates a [`CStr`] from a `[u8]`.
    ///
    /// The provided slice must be `NUL`-terminated, does not contain any
    /// interior `NUL` bytes.
    pub const fn from_bytes_with_nul(bytes: &[u8]) -> Result<&Self, CStrConvertError> {
        if bytes.is_empty() {
            return Err(CStrConvertError::NotNulTerminated);
        }
        if bytes[bytes.len() - 1] != 0 {
            return Err(CStrConvertError::NotNulTerminated);
        }
        let mut i = 0;
        // `i + 1 < bytes.len()` allows LLVM to optimize away bounds checking,
        // while it couldn't optimize away bounds checks for `i < bytes.len() - 1`.
        while i + 1 < bytes.len() {
            if bytes[i] == 0 {
                return Err(CStrConvertError::InteriorNul);
            }
            i += 1;
        }
        // SAFETY: We just checked that all properties hold.
        Ok(unsafe { Self::from_bytes_with_nul_unchecked(bytes) })
    }

    /// Creates a [`CStr`] from a `[u8]`, panic if input is not valid.
    ///
    /// This function is only meant to be used by `c_str!` macro, so
    /// crates using `c_str!` macro don't have to enable `const_panic` feature.
    #[doc(hidden)]
    pub const fn from_bytes_with_nul_unwrap(bytes: &[u8]) -> &Self {
        match Self::from_bytes_with_nul(bytes) {
            Ok(v) => v,
            Err(_) => panic!("string contains interior NUL"),
        }
    }

    /// Creates a [`CStr`] from a `[u8]` without performing any additional
    /// checks.
    ///
    /// # Safety
    ///
    /// `bytes` *must* end with a `NUL` byte, and should only have a single
    /// `NUL` byte (or the string will be truncated).
    #[inline]
    pub const unsafe fn from_bytes_with_nul_unchecked(bytes: &[u8]) -> &CStr {
        // Note: This can be done using pointer deref (which requires
        // `const_raw_ptr_deref` to be const) or `transmute` (which requires
        // `const_transmute` to be const) or `ptr::from_raw_parts` (which
        // requires `ptr_metadata`).
        // While none of them are current stable, it is very likely that one of
        // them will eventually be.
        // SAFETY: Properties of `bytes` guaranteed by the safety precondition.
        unsafe { &*(bytes as *const [u8] as *const Self) }
    }

    /// Returns a C pointer to the string.
    #[inline]
    pub const fn as_char_ptr(&self) -> *const c_types::c_char {
        self.0.as_ptr() as _
    }

    /// Convert the string to a byte slice without the trailing 0 byte.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..self.len()]
    }

    /// Convert the string to a byte slice containing the trailing 0 byte.
    #[inline]
    pub const fn as_bytes_with_nul(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<BStr> for CStr {
    #[inline]
    fn as_ref(&self) -> &BStr {
        self.as_bytes()
    }
}

impl Deref for CStr {
    type Target = BStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl Index<ops::RangeFrom<usize>> for CStr {
    type Output = CStr;

    #[inline]
    fn index(&self, index: ops::RangeFrom<usize>) -> &Self::Output {
        // Delegate bounds checking to slice.
        // Assign to _ to mute clippy's unnecessary operation warning.
        let _ = &self.as_bytes()[index.start..];
        // SAFETY: We just checked the bounds.
        unsafe { Self::from_bytes_with_nul_unchecked(&self.0[index.start..]) }
    }
}

impl Index<ops::RangeFull> for CStr {
    type Output = CStr;

    #[inline]
    fn index(&self, _index: ops::RangeFull) -> &Self::Output {
        self
    }
}

mod private {
    use core::ops;

    //  Marker trait for index types that can be forward to `BStr`.
    pub trait CStrIndex {}

    impl CStrIndex for usize {}
    impl CStrIndex for ops::Range<usize> {}
    impl CStrIndex for ops::RangeInclusive<usize> {}
    impl CStrIndex for ops::RangeToInclusive<usize> {}
}

impl<Idx> Index<Idx> for CStr
where
    Idx: private::CStrIndex,
    BStr: Index<Idx>,
{
    type Output = <BStr as Index<Idx>>::Output;

    #[inline]
    fn index(&self, index: Idx) -> &Self::Output {
        &self.as_bytes()[index]
    }
}

/// Creates a new [`CStr`] from a string literal.
///
/// The string literal should not contain any `NUL` bytes.
///
/// # Examples
///
/// ```
/// # use kernel::c_str;
/// # use kernel::str::CStr;
/// const MY_CSTR: &'static CStr = c_str!("My awesome CStr!");
/// ```
#[macro_export]
macro_rules! c_str {
    ($str:literal) => {{
        const S: &str = concat!($str, "\0");
        const C: &$crate::str::CStr = $crate::str::CStr::from_bytes_with_nul_unwrap(S.as_bytes());
        C
    }};
}

/// A `NUL`-terminated string that is guaranteed to be shorter than a given
/// length. This type is useful because the C side usually imposes a maximum length
/// on types.
///
/// The size parameter `N` represents the maximum number of bytes including `NUL`.
/// This implies that even though `CBoundedStr<0>` is a well-formed type it cannot
/// be safely created.
#[repr(transparent)]
pub struct CBoundedStr<const N: usize>(CStr);

impl<const N: usize> CBoundedStr<N> {
    /// Creates a [`CBoundedStr`] from a [`CStr`].
    ///
    /// The provided [`CStr`] must be shorter than `N`.
    #[inline]
    pub const fn from_c_str(c_str: &CStr) -> Result<&Self, CStrConvertError> {
        if c_str.len_with_nul() > N {
            return Err(CStrConvertError::BoundExceeded);
        }

        // SAFETY: We just checked that all properties hold.
        Ok(unsafe { Self::from_c_str_unchecked(c_str) })
    }

    /// Creates a [`CBoundedStr`] from a [`CStr`] without performing any sanity
    /// checks.
    ///
    /// # Safety
    ///
    /// The provided [`CStr`] must be shorter than `N`.
    #[inline]
    pub const unsafe fn from_c_str_unchecked(c_str: &CStr) -> &Self {
        &*(c_str as *const CStr as *const Self)
    }

    /// Creates a [`CBoundedStr`] from a `[u8]`.
    ///
    /// The provided slice must be `NUL`-terminated, must not contain any
    /// interior `NUL` bytes and must be shorter than `N`.
    #[inline]
    pub fn from_bytes_with_nul(bytes: &[u8]) -> Result<&Self, CStrConvertError> {
        Self::from_c_str(CStr::from_bytes_with_nul(bytes)?)
    }

    /// Creates a [`CBoundedStr`] from a `[u8]` without performing any sanity
    /// checks.
    ///
    /// # Safety
    ///
    /// The provided slice must be `NUL`-terminated, must not contain any
    /// interior `NUL` bytes and must be shorter than `N`.
    #[inline]
    pub const unsafe fn from_bytes_with_nul_unchecked(bytes: &[u8]) -> &Self {
        Self::from_c_str_unchecked(CStr::from_bytes_with_nul_unchecked(bytes))
    }

    /// Creates a [`CBoundedStr`] from a `[u8; N]` without performing any sanity
    /// checks.
    ///
    /// # Safety
    ///
    /// The provided slice must be `NUL`-terminated.
    #[inline]
    pub const unsafe fn from_exact_bytes_with_nul_unchecked(bytes: &[u8; N]) -> &Self {
        Self::from_bytes_with_nul_unchecked(bytes)
    }

    /// Relaxes the bound from `N` to `M`.
    ///
    /// `M` must be no less than the bound `N`.
    #[inline]
    pub const fn relax_bound<const M: usize>(&self) -> &CBoundedStr<M> {
        build_assert!(N <= M, "relaxed bound should be no less than current bound");
        unsafe { CBoundedStr::<M>::from_c_str_unchecked(&self.0) }
    }

    /// Converts the string to a `c_char` array of the same bound, filling
    /// the remaining bytes with zero.
    #[inline]
    pub const fn to_char_array(&self) -> [c_types::c_char; N] {
        let mut ret: [c_types::c_char; N] = [0; N];
        let mut i = 0;
        while i < self.0 .0.len() {
            ret[i] = self.0 .0[i] as _;
            i += 1;
        }
        ret
    }

    /// Expands the string to a `c_char` array of higher bound, filling
    /// the remaining bytes with zero.
    ///
    /// `M` must be no less than the bound `N`.
    #[inline]
    pub const fn expand_to_char_array<const M: usize>(&self) -> [c_types::c_char; M] {
        self.relax_bound().to_char_array()
    }
}

impl<const N: usize> AsRef<BStr> for CBoundedStr<N> {
    #[inline]
    fn as_ref(&self) -> &BStr {
        self.as_bytes()
    }
}

impl<const N: usize> AsRef<CStr> for CBoundedStr<N> {
    #[inline]
    fn as_ref(&self) -> &CStr {
        &self.0
    }
}

impl<const N: usize> Deref for CBoundedStr<N> {
    type Target = CStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Idx, const N: usize> Index<Idx> for CBoundedStr<N>
where
    CStr: Index<Idx>,
{
    type Output = <CStr as Index<Idx>>::Output;

    #[inline]
    fn index(&self, index: Idx) -> &Self::Output {
        &self.0[index]
    }
}

/// Creates a new [`CBoundedStr`] from a string literal.
///
/// The string literal should not contain any `NUL` bytes, and its length with `NUL` should not
/// exceed the bound supplied.
///
/// # Examples
///
/// ```rust,no_run
/// // If no bound is specified, the tighest bound will be inferred:
/// const MY_CSTR: &'static CBoundedStr<17> = c_bounded_str!("My awesome CStr!");
/// ```
///
/// ```rust,compile_fail
/// // This does not compile as the inferred type is `CBoundedStr<17>`.
/// const MY_CSTR: &'static CBoundedStr<100> = c_bounded_str!("My awesome CStr!");
/// ```
///
/// ```rust,no_run
/// // You can relax the bound using the `relax_bound` method.
/// const MY_CSTR: &'static CBoundedStr<100> = c_bounded_str!("My awesome CStr!").relax_bound();
///
/// // Or alternatively specify a bound.
/// // In this case the supplied bound must be a constant expression.
/// const MY_CSTR2: &'static CBoundedStr<100> = c_bounded_str!(100, "My awesome CStr!");
///
/// // Or let the compiler infer the bound for you.
/// const MY_CSTR3: &'static CBoundedStr<100> = c_bounded_str!(_, "My awesome CStr!");
/// ```
///
/// ```rust,compile_fail
/// // These do not compile as the string is longer than the specified bound.
/// const MY_CSTR: &'static CBoundedStr<4> = c_bounded_str!(4, "My awesome CStr!");
/// const MY_CSTR2: &'static CBoundedStr<4> = c_bounded_str!(_, "My awesome CStr!");
/// ```
#[macro_export]
macro_rules! c_bounded_str {
    ($str:literal) => {{
        const S: &$crate::str::CStr = $crate::c_str!($str);
        const C: &$crate::str::CBoundedStr<{ S.len_with_nul() }> =
            unsafe { $crate::str::CBoundedStr::from_c_str_unchecked(S) };
        C
    }};
    (_, $str:literal) => {{
        $crate::c_bounded_str!($str).relax_bound()
    }};
    ($bound:expr, $str:literal) => {{
        const C: &$crate::str::CBoundedStr<{ $bound }> = $crate::c_bounded_str!($str).relax_bound();
        C
    }};
}
