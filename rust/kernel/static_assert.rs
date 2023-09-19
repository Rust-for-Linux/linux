// SPDX-License-Identifier: GPL-2.0

//! Static assert.

/// Static assert (i.e. compile-time assert).
///
/// There are several forms of this macro:
///
///   - Boolean assertion: `expr`.
///   - Set membership assertion: `(expr) is in {a0, ..., aN}`.
///   - Interval membership assertion: `(expr) is in [min, max]`.
///   - Fits-in-type assertion: `(expr) fits in type`.
///
/// The expressions in all the forms are evaluated in [const context].
///
/// [const context]: https://doc.rust-lang.org/reference/const_eval.html
///
/// # Boolean assertion: `expr`
///
/// Statically asserts the given expression.
///
/// Similar to C11 [`_Static_assert`] and C++11 [`static_assert`].
/// The feature may be added to Rust in the future: see [RFC 2790].
///
/// [`_Static_assert`]: https://en.cppreference.com/w/c/language/_Static_assert
/// [`static_assert`]: https://en.cppreference.com/w/cpp/language/static_assert
/// [RFC 2790]: https://github.com/rust-lang/rfcs/issues/2790
///
/// ## Examples
///
/// ```
/// // Trivial assert.
/// static_assert!(42 > 24);
///
/// // Assert on sizes, similar to C's `sizeof(T)`.
/// static_assert!(core::mem::size_of::<u8>() == 1);
///
/// // Assert on binary string.
/// const X: &[u8] = b"bar";
/// static_assert!(X[1] == b'a');
///
/// // Check we uphold some constraint from the C side by testing the bindings.
/// static_assert!(RUST_BUFFER_SIZE >= bindings::LOG_LINE_MAX);
///
/// // Calling `const fn`s is possible.
/// const fn f(x: i32) -> i32 {
///     x + 2
/// }
/// static_assert!(f(40) == 42);
/// ```
///
/// # Set membership assertion: `(expr) is in {a0, ..., aN}`
///
/// Statically asserts that the given expression (typically a `const` integer) is in a set.
/// The negated form (`is not in`) is also available.
///
/// ## Examples
///
/// ```
/// // Trivial usage.
/// static_assert!((-2) is not in {-1, 0, 2});
/// static_assert!((-1) is     in {-1, 0, 2});
/// static_assert!(( 0) is     in {-1, 0, 2});
/// static_assert!(( 1) is not in {-1, 0, 2});
/// static_assert!(( 2) is     in {-1, 0, 2});
/// static_assert!(( 3) is not in {-1, 0, 2});
///
/// // Typical usage.
/// static_assert!((SOME_CONSTANT_DEPENDING_ON_ARCH) is in {FOO, BAR, BAZ});
/// static_assert!((core::mem::size_of::<usize>()) is in {4, 8});
/// ```
///
/// # Interval membership assertion: `(expr) is in [min, max]`
///
/// Statically asserts that the given expression (typically a `const` integer) is in a closed
/// interval (i.e. inclusive range). The negated form (`is not in`) is also available.
///
/// ## Examples
///
/// ```
/// // Trivial usage.
/// static_assert!((-2) is not in [-1, 2]);
/// static_assert!((-1) is     in [-1, 2]);
/// static_assert!(( 0) is     in [-1, 2]);
/// static_assert!(( 1) is     in [-1, 2]);
/// static_assert!(( 2) is     in [-1, 2]);
/// static_assert!(( 3) is not in [-1, 2]);
///
/// // Typical usage.
/// static_assert!((FOO) is in [MIN_FOO, MAX_FOO]);
/// ```
///
/// # Fits-in-type assertion: `(expr) fits in type`
///
/// Statically asserts that the given expression (typically a `const` integer) fits in the given
/// type (which must provide `T::MIN` and `T::MAX`). The negated form (`does not fit in`) is also
/// available.
///
/// Casting a "kernel integer" (i.e. up to [`i64`]/[`u64`]) to [`i128`] within the expression is
/// allowed to easily manipulate integers: no 128-bit code will be generated since it will be
/// evaluated in a const context.
///
/// ## Examples
///
/// ```
/// // Trivial usage.
/// static_assert!(( -1) does not fit  in u8);
/// static_assert!((  0)          fits in u8);
/// static_assert!((255)          fits in u8);
/// static_assert!((256) does not fit  in u8);
///
/// // Two's complement.
/// static_assert!((-128)          fits in i8);
/// static_assert!(( 127)          fits in i8);
/// static_assert!(( 128) does not fit  in i8);
///
/// // Using `i128` for easy manipulation of integers.
/// const MAX_ERRNO: u32 = 4095;
/// static_assert!((-(MAX_ERRNO as i128)) fits in i16);
/// ```
#[macro_export]
macro_rules! static_assert {
    // Boolean assertion: `expr`.
    ($condition:expr) => {
        const _: () = core::assert!($condition);
    };

    // Set membership assertion: `(expr) is in {a0, ..., aN}`.
    (($expression:expr) is in {$($a:expr),+}) => {
        static_assert!(  $(($expression) == ($a))||* );
    };
    (($expression:expr) is not in {$($a:expr),+}) => {
        static_assert!(!($(($expression) == ($a))||*));
    };

    // Interval membership assertion: `(expr) is in [min, max]`.
    (($expression:expr) is in [$min:expr, $max:expr]) => {
        static_assert!(  ($expression) >= ($min) && ($expression) <= ($max) );
    };
    (($expression:expr) is not in [$min:expr, $max:expr]) => {
        static_assert!(!(($expression) >= ($min) && ($expression) <= ($max)));
    };

    // Fits-in-type assertion: `(expr) fits in type`.
    (($expression:expr) fits in $t:ty) => {
        static_assert!(($expression) is     in [<$t>::MIN as i128, <$t>::MAX as i128]);
    };
    (($expression:expr) does not fit in $t:ty) => {
        static_assert!(($expression) is not in [<$t>::MIN as i128, <$t>::MAX as i128]);
    };
}

// Tests.
//
// These should later on go into a proper test.

static_assert!(42 > 24);
static_assert!(core::mem::size_of::<u8>() == 1);

const X: &[u8] = b"bar";
static_assert!(X[1] == b'a');

const fn f(x: i32) -> i32 {
    x + 2
}
static_assert!(f(40) == 42);

static_assert!((-2) is not in {-1, 0, 2});
static_assert!((-1) is     in {-1, 0, 2});
static_assert!(( 0) is     in {-1, 0, 2});
static_assert!(( 1) is not in {-1, 0, 2});
static_assert!(( 2) is     in {-1, 0, 2});
static_assert!(( 3) is not in {-1, 0, 2});

static_assert!((core::mem::size_of::<usize>()) is in {4, 8});

static_assert!((-2) is not in [-1, 2]);
static_assert!((-1) is     in [-1, 2]);
static_assert!(( 0) is     in [-1, 2]);
static_assert!(( 1) is     in [-1, 2]);
static_assert!(( 2) is     in [-1, 2]);
static_assert!(( 3) is not in [-1, 2]);

static_assert!((-129) does not fit  in i8);
static_assert!((-128)          fits in i8);
static_assert!(( 127)          fits in i8);
static_assert!(( 128) does not fit  in i8);

static_assert!(( -1) does not fit  in u8);
static_assert!((  0)          fits in u8);
static_assert!((255)          fits in u8);
static_assert!((256) does not fit  in u8);

const MAX_ERRNO: u32 = 4095;
static_assert!((-(MAX_ERRNO as i128))          fits in i16);
static_assert!((-(MAX_ERRNO as i128)) does not fit  in i8);
static_assert!((-(MAX_ERRNO as i128)) does not fit  in u16);
