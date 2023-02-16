// SPDX-License-Identifier: GPL-2.0

//! XArray abstraction.
//!
//! C header: [`include/linux/xarray.h`](../../include/linux/xarray.h)

use crate::{
    bindings,
    error::{Error, Result},
    types::{ForeignOwnable, Opaque, ScopeGuard},
};
use core::{marker::PhantomData, ops::Deref};

/// Flags passed to `XArray::new` to configure the `XArray`.
type Flags = bindings::gfp_t;

/// Flag values passed to `XArray::new` to configure the `XArray`.
pub mod flags {
    /// Use IRQ-safe locking
    pub const LOCK_IRQ: super::Flags = bindings::BINDINGS_XA_FLAGS_LOCK_IRQ;
    /// Use softirq-safe locking
    pub const LOCK_BH: super::Flags = bindings::BINDINGS_XA_FLAGS_LOCK_BH;
    /// Track which entries are free (distinct from None)
    pub const TRACK_FREE: super::Flags = bindings::BINDINGS_XA_FLAGS_TRACK_FREE;
    /// Initialize array index 0 as busy
    pub const ZERO_BUSY: super::Flags = bindings::BINDINGS_XA_FLAGS_ZERO_BUSY;
    /// Use GFP_ACCOUNT for internal memory allocations
    pub const ACCOUNT: super::Flags = bindings::BINDINGS_XA_FLAGS_ACCOUNT;
    /// Create an allocating `XArray` starting at index 0
    pub const ALLOC: super::Flags = bindings::BINDINGS_XA_FLAGS_ALLOC;
    /// Create an allocating `XArray` starting at index 1
    pub const ALLOC1: super::Flags = bindings::BINDINGS_XA_FLAGS_ALLOC1;
}

/// Wrapper for a value owned by the `XArray` which holds the `XArray` lock until dropped.
///
/// # Invariants
///
/// The `*mut T` is always non-NULL and owned by the referenced `XArray`
pub struct Guard<'a, T: ForeignOwnable>(*mut T, &'a Opaque<bindings::xarray>);

impl<'a, T: ForeignOwnable> Guard<'a, T> {
    /// Borrow the underlying value wrapped by the `Guard`.
    ///
    /// Returns a `T::Borrowed` type for the owned `ForeignOwnable` type.
    pub fn borrow<'b>(&'b self) -> T::Borrowed<'b>
    where
        'a: 'b,
    {
        // SAFETY: The value is owned by the `XArray`, the lifetime it is borrowed for must not
        // outlive the `XArray` itself, nor the Guard that holds the lock ensuring the value
        // remains in the `XArray`.
        unsafe { T::borrow(self.0 as _) }
    }
}

// Convenience impl for `ForeignOwnable` types whose `Borrowed`
// form implements Deref.
impl<'a, T: ForeignOwnable> Deref for Guard<'a, T>
where
    T::Borrowed<'static>: Deref,
{
    type Target = <T::Borrowed<'static> as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        // SAFETY: See the `borrow()` method. The dereferenced `T::Borrowed` value
        // must share the same lifetime, so we can return a reference to it.
        // TODO: Is this really sound?
        unsafe { &*(T::borrow(self.0 as _).deref() as *const _) }
    }
}

impl<'a, T: ForeignOwnable> Drop for Guard<'a, T> {
    fn drop(&mut self) {
        // SAFETY: The XArray we have a reference to owns the C xarray object.
        unsafe { bindings::xa_unlock(self.1.get()) };
    }
}

/// Represents a reserved slot in an `XArray`, which does not yet have a value but has an assigned
/// index and may not be allocated by any other user. If the Reservation is dropped without
/// being filled, the entry is marked as available again.
///
/// Users must ensure that reserved slots are not filled by other mechanisms, or otherwise their
/// contents may be dropped and replaced (which will print a warning).
pub struct Reservation<'a, T: ForeignOwnable>(&'a XArray<T>, usize, PhantomData<T>);

impl<'a, T: ForeignOwnable> Reservation<'a, T> {
    /// Store a value into the reserved slot.
    pub fn store(self, value: T) -> Result<usize> {
        if self.0.replace(self.1, value)?.is_some() {
            crate::pr_err!("XArray: Reservation stored but the entry already had data!\n");
            // Consider it a success anyway, not much we can do
        }
        let index = self.1;
        core::mem::forget(self);
        Ok(index)
    }

    /// Returns the index of this reservation.
    pub fn index(&self) -> usize {
        self.1
    }
}

impl<'a, T: ForeignOwnable> Drop for Reservation<'a, T> {
    fn drop(&mut self) {
        if self.0.remove(self.1).is_some() {
            crate::pr_err!("XArray: Reservation dropped but the entry was not empty!\n");
        }
    }
}

/// An array which efficiently maps sparse integer indices to owned objects.
///
/// This is similar to a `Vec<Option<T>>`, but more efficient when there are holes in the
/// index space, and can be efficiently grown.
///
/// This structure is expected to often be used with an inner type that can either be efficiently
/// cloned, such as an `Arc<T>`.
pub struct XArray<T: ForeignOwnable> {
    xa: Opaque<bindings::xarray>,
    _p: PhantomData<T>,
}

impl<T: ForeignOwnable> XArray<T> {
    /// Creates a new `XArray` with the given flags.
    pub fn new(flags: Flags) -> Result<XArray<T>> {
        let xa = Opaque::uninit();

        // SAFETY: We have just created `xa`. This data structure does not require
        // pinning.
        unsafe { bindings::xa_init_flags(xa.get(), flags) };

        // INVARIANT: Initialize the `XArray` with a valid `xa`.
        Ok(XArray {
            xa,
            _p: PhantomData,
        })
    }

    /// Replaces an entry with a new value, returning the old value (if any).
    pub fn replace(&self, index: usize, value: T) -> Result<Option<T>> {
        let new = value.into_foreign();
        let guard = ScopeGuard::new(|| unsafe {
            T::from_foreign(new);
        });

        let old = unsafe {
            bindings::xa_store(
                self.xa.get(),
                index.try_into()?,
                new as *mut _,
                bindings::GFP_KERNEL,
            )
        };

        let err = unsafe { bindings::xa_err(old) };
        if err != 0 {
            Err(Error::from_kernel_errno(err))
        } else if old.is_null() {
            guard.dismiss();
            Ok(None)
        } else {
            guard.dismiss();
            Ok(Some(unsafe { T::from_foreign(old) }))
        }
    }

    /// Replaces an entry with a new value, dropping the old value (if any).
    pub fn set(&self, index: usize, value: T) -> Result {
        self.replace(index, value)?;
        Ok(())
    }

    /// Looks up and returns a reference to an entry in the array, returning a `Guard` if it
    /// exists.
    ///
    /// This guard blocks all other actions on the `XArray`. Callers are expected to drop the
    /// `Guard` eagerly to avoid blocking other users, such as by taking a clone of the value.
    pub fn get(&self, index: usize) -> Option<Guard<'_, T>> {
        // SAFETY: `self.xa` is always valid by the type invariant.
        let p = unsafe {
            bindings::xa_lock(self.xa.get());
            bindings::xa_load(self.xa.get(), index.try_into().ok()?)
        };

        if p.is_null() {
            unsafe { bindings::xa_unlock(self.xa.get()) };
            None
        } else {
            Some(Guard(p as _, &self.xa))
        }
    }

    /// Removes and returns an entry, returning it if it existed.
    pub fn remove(&self, index: usize) -> Option<T> {
        let p = unsafe { bindings::xa_erase(self.xa.get(), index.try_into().ok()?) };
        if p.is_null() {
            None
        } else {
            Some(unsafe { T::from_foreign(p) })
        }
    }

    /// Allocate a new index in the array, optionally storing a new value into it, with
    /// configurable bounds for the index range to allocate from.
    ///
    /// If `value` is `None`, then the index is reserved from further allocation but remains
    /// free for storing a value into it.
    pub fn alloc_limits(&self, value: Option<T>, min: u32, max: u32) -> Result<usize> {
        let new = value.map_or(core::ptr::null(), |a| a.into_foreign());
        let mut id: u32 = 0;

        // SAFETY: `self.xa` is always valid by the type invariant. If this succeeds, it
        // takes ownership of the passed `T` (if any). If it fails, we must drop the
        // `T` again.
        let ret = unsafe {
            bindings::xa_alloc(
                self.xa.get(),
                &mut id,
                new as *mut _,
                bindings::xa_limit { min, max },
                bindings::GFP_KERNEL,
            )
        };

        if ret < 0 {
            // Make sure to drop the value we failed to store
            if !new.is_null() {
                // SAFETY: If `new` is not NULL, it came from the `ForeignOwnable` we got
                // from the caller.
                unsafe { T::from_foreign(new) };
            }
            Err(Error::from_kernel_errno(ret))
        } else {
            Ok(id as usize)
        }
    }

    /// Allocate a new index in the array, optionally storing a new value into it.
    ///
    /// If `value` is `None`, then the index is reserved from further allocation but remains
    /// free for storing a value into it.
    pub fn alloc(&self, value: Option<T>) -> Result<usize> {
        self.alloc_limits(value, 0, u32::MAX)
    }

    /// Reserve a new index in the array within configurable bounds for the index.
    ///
    /// Returns a `Reservation` object, which can then be used to store a value at this index or
    /// otherwise free it for reuse.
    pub fn reserve_limits(&self, min: u32, max: u32) -> Result<Reservation<'_, T>> {
        Ok(Reservation(
            self,
            self.alloc_limits(None, min, max)?,
            PhantomData,
        ))
    }

    /// Reserve a new index in the array.
    ///
    /// Returns a `Reservation` object, which can then be used to store a value at this index or
    /// otherwise free it for reuse.
    pub fn reserve(&self) -> Result<Reservation<'_, T>> {
        Ok(Reservation(self, self.alloc(None)?, PhantomData))
    }
}

impl<T: ForeignOwnable> Drop for XArray<T> {
    fn drop(&mut self) {
        // SAFETY: `self.xa` is valid by the type invariant, and as we have the only reference to
        // the `XArray` we can safely iterate its contents and drop everything.
        unsafe {
            let mut index: core::ffi::c_ulong = 0;
            let mut entry = bindings::xa_find(
                self.xa.get(),
                &mut index,
                core::ffi::c_ulong::MAX,
                bindings::BINDINGS_XA_PRESENT,
            );
            while !entry.is_null() {
                T::from_foreign(entry);
                entry = bindings::xa_find_after(
                    self.xa.get(),
                    &mut index,
                    core::ffi::c_ulong::MAX,
                    bindings::BINDINGS_XA_PRESENT,
                );
            }

            bindings::xa_destroy(self.xa.get());
        }
    }
}

// SAFETY: XArray is thread-safe and all mutation operations are internally locked.
unsafe impl<T: Send + ForeignOwnable> Send for XArray<T> {}
unsafe impl<T: Sync + ForeignOwnable> Sync for XArray<T> {}
