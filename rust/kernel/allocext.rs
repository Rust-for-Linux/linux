//! Allocator Extensions.

use alloc::alloc::{AllocError, Allocator, Layout};
use alloc::boxed::Box;
use core::ptr::{self, NonNull};

use crate::bindings;

/// Allocator extension to pass Flags to allocator.
pub trait FlagAllocator: Allocator {
    /// Allocated memory with flag
    fn allocate_with_flag(
        &self,
        layout: Layout,
        flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError>;
}

#[cfg(not(test))]
#[cfg(not(testlib))]
impl FlagAllocator for alloc::alloc::Global {
    fn allocate_with_flag(
        &self,
        layout: Layout,
        flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError> {
        // `krealloc()` is used instead of `kmalloc()` because the latter is
        // an inline function and cannot be bound to as a result.
        let mem = unsafe { bindings::krealloc(ptr::null(), layout.size(), flags) as *mut u8 };
        if mem.is_null() {
            return Err(AllocError);
        }
        let mem = unsafe { core::slice::from_raw_parts_mut(mem, bindings::ksize(mem as _)) };
        // Safety: checked for non null abpve
        Ok(unsafe { NonNull::new_unchecked(mem) })
    }
}

#[cfg(test)]
#[cfg(testlib)]
impl FlagAllocator for alloc::alloc::Global {
    fn allocate_with_flag(
        &self,
        layout: Layout,
        _flags: bindings::gfp_t,
    ) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate(layout)
    }
}

// Box Ext
/// Box extension Proiding functions to pass GFP flags to the allocator.
pub trait BoxAllocFlagInExt<T: Sized, A: FlagAllocator> {
    /// Allocate unint box with flags.
    fn try_new_uninit_flag_in(
        flags: bindings::gfp_t,
        alloc: A,
    ) -> Result<Box<core::mem::MaybeUninit<T>, A>, AllocError>;

    /// Allocate box with flags.
    fn try_new_flag_in(x: T, flags: bindings::gfp_t, alloc: A) -> Result<Box<T, A>, AllocError> {
        let mut boxed = Self::try_new_uninit_flag_in(flags, alloc)?;
        unsafe {
            boxed.as_mut_ptr().write(x);
            Ok(boxed.assume_init())
        }
    }
}

/// Box extension Providing function to pass GFP flags to the Global Allocator.
pub trait BoxAllocFlagExt<T: Sized>: BoxAllocFlagInExt<T, alloc::alloc::Global> {
    /// Allocated uniit box with flags on Global Allocator.
    fn try_new_uninit_flag(
        flags: bindings::gfp_t,
    ) -> Result<Box<core::mem::MaybeUninit<T>>, AllocError> {
        Self::try_new_uninit_flag_in(flags, alloc::alloc::Global)
    }

    /// Allocated box with flags on Glabal Allocator.
    fn tr_new_flag(x: T, flags: bindings::gfp_t) -> Result<Box<T>, AllocError> {
        Self::try_new_flag_in(x, flags, alloc::alloc::Global)
    }
}

impl<T, A> BoxAllocFlagInExt<T, A> for Box<T, A>
where
    T: Sized,
    A: FlagAllocator,
{
    fn try_new_uninit_flag_in(
        flags: bindings::gfp_t,
        alloc: A,
    ) -> Result<Box<core::mem::MaybeUninit<T>, A>, AllocError> {
        let layout = Layout::new::<core::mem::MaybeUninit<T>>();
        let ptr = alloc.allocate_with_flag(layout, flags)?.cast();
        unsafe { Ok(Box::from_raw_in(ptr.as_ptr(), alloc)) }
    }
}

impl<T: Sized> BoxAllocFlagExt<T> for Box<T> {}
