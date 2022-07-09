// SPDX-License-Identifier: GPL-2.0

//! Extension trait for `Box`
//!

use alloc::alloc::AllocError;
use alloc::boxed::Box;
use core::mem::MaybeUninit;

use crate::bindings;

/// Extension trait for `Box`
pub trait BoxExt<T: ?Sized> {
    /// Tries to allocate a new box atomically and place `x` into it, returning an error if the allocation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use kernel::box_ext::BoxExt;
    /// let my_box = Box::try_new_atomic(101).unwrap();
    /// # // It is not currently possible to return Result form doctest, hence the unwrap
    /// # //let my_box = Box::try_alloc_atomic(101)?;
    /// ```
    fn try_new_atomic(x: T) -> Result<Self, AllocError>
    where
        Self: Sized;
}

#[cfg(not(test))]
#[cfg(not(testlib))]
impl<T> BoxExt<T> for Box<T> {
    fn try_new_atomic(x: T) -> Result<Box<T>, AllocError> {
        let layout = core::alloc::Layout::new::<core::mem::MaybeUninit<T>>();
        let ptr = crate::allocator::ALLOCATOR
            .allocate_with_flags(layout, bindings::GFP_ATOMIC)?
            .cast();
        let mut boxed: Box<MaybeUninit<T>> =
            unsafe { Box::from_raw_in(ptr.as_ptr(), alloc::alloc::Global) };

        unsafe {
            boxed.as_mut_ptr().write(x);
            Ok(boxed.assume_init())
        }
    }
}
