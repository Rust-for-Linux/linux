// SPDX-License-Identifier: GPL-2.0

//! Alignment helper functions.
//!
//! This is a Rust version of 'align.h' file.

/// Return aligned pointer of `ptr` to `T` according to the `alignment`
/// requirement.
#[allow(dead_code)]
#[inline]
pub fn ptr_align<T>(ptr: *const T, alignment: usize) -> *const T {
    let ptr_val: usize = ptr as usize;
    align(ptr_val, alignment) as *const T
}

/// Return downward aligned pointer of `ptr` to `T` according to the `alignment`
/// requirement.
#[allow(dead_code)]
#[inline]
pub fn ptr_align_down<T>(ptr: *const T, alignment: usize) -> *const T {
    let ptr_val: usize = ptr as usize;
    align_down(ptr_val, alignment) as *const T
}

/// Return the alignment of `val` according to `mask`.
///
/// `mask` must have continuous '1' which are little-end aligned.
///
/// This function is const and can be used in const context.
#[allow(dead_code)]
#[inline]
pub const fn mask(val: usize, mask: usize) -> usize {
    (val + mask) & !(mask)
}

/// Return the alignment of `alignment` bytes of `val`.
///
/// `alignment` must be a power of 2, and this function doesn't verify the
/// requirement. It is the user's responsibility to take charge of this.
///
/// This function is const and can be used in const context.
#[allow(dead_code)]
#[inline]
pub const fn align(val: usize, alignment: usize) -> usize {
    let mask_val: usize = alignment - 1;
    mask(val, mask_val)
}

/// Return the alignment downward of `alignment` bytes of `val`.
///
/// `alignment` must be a power of 2, and this function doesn't verify the
/// requirement. It is the user's responsibility to take charge of this.
///
/// This function is const and can be used in const context.
#[allow(dead_code)]
#[inline]
pub const fn align_down(val: usize, alignment: usize) -> usize {
    let val: usize = val - (alignment - 1);
    align(val, alignment)
}

/// Test if `val` is `alignment` bytes aligned.
///
/// `alignment` must be a power of 2, and this function doesn't verify the
/// requirement. It is the user's responsibility to take charge of this.
///
/// This function is const and can be used in const context.
#[allow(dead_code)]
#[inline]
pub const fn is_aligned(val: usize, alignment: usize) -> bool {
    (val & (alignment - 1)) == 0
}

/// An interface for dealing with alignment.
trait Align {
    /// The type of alignment value. This type should be integer.
    /// TODO: Bound this type to integer only.
    type Alignment;

    /// Alignment by mask
    fn mask(&self, mask: Self::Alignment) -> Self;

    /// Alignment by bytes.
    fn align(&self, alignment: Self::Alignment) -> Self;

    /// Alignment downward by bytes.
    fn align_down(&self, alignment: Self::Alignment) -> Self;

    /// Test if Aligned.
    fn is_aligned(&self, alignment: Self::Alignment) -> bool;
}

/// A Helper macro for implementing `Align` trait for primitive integer types.
macro_rules! impl_align_for_integer {
    ($type: ty) => {
        impl Align for $type {
            type Alignment = Self;

            #[inline]
            fn mask(&self, mask: Self) -> Self {
                (self + mask) & !(mask)
            }

            #[inline]
            fn align(&self, alignment: Self) -> Self {
                let mask = alignment - 1;
                self.mask(mask)
            }

            #[inline]
            fn align_down(&self, alignment: Self) -> Self {
                let val: Self = self - (alignment - 1);
                val.align(alignment)
            }

            #[inline]
            fn is_aligned(&self, alignment: Self) -> bool {
                (self & (alignment - 1)) == 0
            }
        }
    };
}

impl_align_for_integer!(u8);
impl_align_for_integer!(u16);
impl_align_for_integer!(u32);
impl_align_for_integer!(u64);
impl_align_for_integer!(usize);

impl_align_for_integer!(i8);
impl_align_for_integer!(i16);
impl_align_for_integer!(i32);
impl_align_for_integer!(i64);
impl_align_for_integer!(isize);

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_SIZE: usize = 4096;

    #[test]
    fn test_const_fn() {
        assert_eq!(mask((PAGE_SIZE * 2) as usize, 0x03), PAGE_SIZE * 2);
        assert_eq!(mask((PAGE_SIZE * 2) as usize, 0x07), PAGE_SIZE * 2);
        assert_eq!(mask((PAGE_SIZE * 2) as usize, PAGE_SIZE - 1), PAGE_SIZE * 2);
        assert_eq!(align((PAGE_SIZE * 2) as usize, 4), PAGE_SIZE * 2);
        assert_eq!(align((PAGE_SIZE * 2) as usize, 8), PAGE_SIZE * 2);
        assert_eq!(align((PAGE_SIZE * 2) as usize, PAGE_SIZE), PAGE_SIZE * 2);
        assert_eq!(align_down((PAGE_SIZE * 2) as usize, 4), PAGE_SIZE * 2);
        assert_eq!(align_down((PAGE_SIZE * 2) as usize, 8), PAGE_SIZE * 2);
        assert_eq!(
            align_down((PAGE_SIZE * 2) as usize, PAGE_SIZE),
            PAGE_SIZE * 2
        );

        assert_eq!(mask((PAGE_SIZE * 2 - 2) as usize, 0x03), PAGE_SIZE * 2);
        assert_eq!(mask((PAGE_SIZE * 2 - 4) as usize, 0x07), PAGE_SIZE * 2);
        assert_eq!(
            mask((PAGE_SIZE * 2 - 512) as usize, PAGE_SIZE - 1),
            PAGE_SIZE * 2
        );
        assert_eq!(align((PAGE_SIZE * 2 - 2) as usize, 4), PAGE_SIZE * 2);
        assert_eq!(align((PAGE_SIZE * 2 - 4) as usize, 8), PAGE_SIZE * 2);
        assert_eq!(
            align((PAGE_SIZE * 2 - 512) as usize, PAGE_SIZE),
            PAGE_SIZE * 2
        );
        assert_eq!(
            align_down((PAGE_SIZE * 2 - 2) as usize, 4),
            PAGE_SIZE * 2 - 4
        );
        assert_eq!(
            align_down((PAGE_SIZE * 2 - 4) as usize, 8),
            PAGE_SIZE * 2 - 8
        );
        assert_eq!(
            align_down((PAGE_SIZE * 2 - 512) as usize, PAGE_SIZE),
            PAGE_SIZE
        );
    }

    macro_rules! test_for_integer {
        ($type: ty, $val: expr) => {
            let val: $type = $val;
            assert_eq!((val).mask(4 - 1), val);
            assert_eq!((val).align(4), val);
            assert_eq!((val).align_down(4), val);
            assert_eq!(val.is_aligned(4), true);

            assert_eq!((val - 1).mask(4 - 1), val);
            assert_eq!((val - 1).align(4), val);
            assert_eq!((val - 1).align_down(4), val - 4);
            assert_eq!((val - 1).is_aligned(4), false);
        };
    }

    #[test]
    #[allow(overflowing_literals)]
    fn test_integer_fn() {
        test_for_integer!(u8, 0xf0);
        test_for_integer!(i8, 0xf0 as i8);
        test_for_integer!(u16, 0xf000);
        test_for_integer!(i16, 0xf000 as i16);
        test_for_integer!(u32, 0xf0000000);
        test_for_integer!(i32, 0xf0000000 as i32);
        test_for_integer!(u64, 0xf000000000000000);
        test_for_integer!(i64, 0xf000000000000000 as i64);
        test_for_integer!(usize, 0xf0);
        test_for_integer!(isize, 0xf0 as isize);
    }
}
