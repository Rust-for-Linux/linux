// SPDX-License-Identifier: GPL-2.0

//! A reference-counted pointer.
//!
//! This module implements a way for users to create reference-counted objects and pointers to
//! them. Such a pointer automatically increments and decrements the count, and drops the
//! underlying object when it reaches zero. It is also safe to use concurrently from multiple
//! threads.
//!
//! It is different from the standard library's [`Arc`] in two ways: it does not support weak
//! references, which allows it to be smaller -- a single pointer-sized integer; it allows users to
//! safely increment the reference count from a single reference to the underlying object.
//!
//! [`Arc`]: https://doc.rust-lang.org/std/sync/struct.Arc.html

use crate::Result;
use alloc::boxed::Box;
use core::{
    mem::ManuallyDrop,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{fence, AtomicUsize, Ordering},
};

/// A reference-counted pointer to an instance of `T`.
///
/// The reference count is incremented when new instances of [`Ref`] are created, and decremented
/// when they are dropped. When the count reaches zero, the underlying `T` is also dropped.
///
/// # Invariants
///
/// The value stored in [`RefCounted::get_count`] corresponds to the number of instances of [`Ref`]
/// that point to that instance of `T`.
pub struct Ref<T: RefCounted + ?Sized> {
    ptr: NonNull<T>,
}

// SAFETY: It is safe to send `Ref<T>` to another thread when the underlying `T` is `Sync` because
// it effectively means sharing `&T` (which is safe because `T` is `Sync`); additionally, it needs
// `T` to be `Send` because any thread that has a `Ref<T>` may ultimately access `T` directly, for
// example, when the reference count reaches zero and `T` is dropped.
unsafe impl<T: RefCounted + ?Sized + Sync + Send> Send for Ref<T> {}

// SAFETY: It is safe to send `&Ref<T>` to another thread when the underlying `T` is `Sync` for
// the same reason as above. `T` needs to be `Send` as well because a thread can clone a `&Ref<T>`
// into a `Ref<T>`, which may lead to `T` being accessed by the same reasoning as above.
unsafe impl<T: RefCounted + ?Sized + Sync + Send> Sync for Ref<T> {}

impl<T: RefCounted> Ref<T> {
    /// Constructs a new reference counted instance of `T`.
    pub fn try_new(contents: T) -> Result<Self> {
        let boxed = Box::try_new(contents)?;
        boxed.get_count().count.store(1, Ordering::Relaxed);
        let ptr = NonNull::from(Box::leak(boxed));
        Ok(Ref { ptr })
    }
}

impl<T: RefCounted + ?Sized> Ref<T> {
    /// Creates a new reference-counted pointer to the given instance of `T`.
    ///
    /// It works by incrementing the current reference count as part of constructing the new
    /// pointer.
    pub fn new_from(obj: &T) -> Self {
        let ref_count = obj.get_count();
        let cur = ref_count.count.fetch_add(1, Ordering::Relaxed);
        if cur == usize::MAX {
            panic!("Reference count overflowed");
        }
        Self {
            ptr: NonNull::from(obj),
        }
    }

    /// Returns a mutable reference to `T` iff the reference count is one. Otherwise returns
    /// [`None`].
    pub fn get_mut(&mut self) -> Option<&mut T> {
        // Synchronises with the decrement in `drop`.
        if self.get_count().count.load(Ordering::Acquire) != 1 {
            return None;
        }
        // SAFETY: Since there is only one reference, we know it isn't possible for another thread
        // to concurrently call this.
        Some(unsafe { self.ptr.as_mut() })
    }

    /// Determines if two reference-counted pointers point to the same underlying instance of `T`.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        core::ptr::eq(a.ptr.as_ptr(), b.ptr.as_ptr())
    }

    /// Deconstructs a [`Ref`] object into a raw pointer.
    ///
    /// It can be reconstructed once via [`Ref::from_raw`].
    pub fn into_raw(obj: Self) -> *const T {
        let no_drop = ManuallyDrop::new(obj);
        no_drop.ptr.as_ptr()
    }

    /// Recreates a [`Ref`] instance previously deconstructed via [`Ref::into_raw`].
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by a previous call to [`Ref::into_raw`]. Additionally, it
    /// can only be called once for each previous call to [``Ref::into_raw`].
    pub unsafe fn from_raw(ptr: *const T) -> Self {
        Ref {
            ptr: if cfg!(debug_assertions) {
                NonNull::new(ptr as _).unwrap()
            } else {
                // SAFETY: since `ptr` must have been returned by a previous call to `Ref::into_raw`
                // it cannot be null.
                NonNull::new_unchecked(ptr as _)
            },
        }
    }
}

impl<T: RefCounted + ?Sized> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: By the type invariant, there is necessarily a reference to the object, so it is
        // safe to dereference it.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: RefCounted + ?Sized> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Self::new_from(self)
    }
}

impl<T: RefCounted + ?Sized> Drop for Ref<T> {
    fn drop(&mut self) {
        {
            // SAFETY: By the type invariant, there is necessarily a reference to the object.
            let obj = unsafe { self.ptr.as_ref() };

            // Synchronises with the acquire below or with the acquire in `get_mut`.
            if obj.get_count().count.fetch_sub(1, Ordering::Release) != 1 {
                return;
            }
        }

        // Synchronises with the release when decrementing above. This ensures that modifications
        // from all previous threads/CPUs are visible to the underlying object's `drop`.
        fence(Ordering::Acquire);

        // The count reached zero, we must free the memory.
        //
        // SAFETY: The pointer was initialised from the result of `Box::into_raw`.
        unsafe { Box::from_raw(self.ptr.as_ptr()) };
    }
}

/// Trait for reference counted objects.
///
/// # Safety
///
/// Implementers of [`RefCounted`] must ensure that all of their constructors call
/// [`Ref::try_new`].
pub unsafe trait RefCounted {
    /// Returns a pointer to the object field holds the reference count.
    fn get_count(&self) -> &RefCount;
}

/// Holds the reference count of an object.
///
/// It is meant to be embedded in objects to be reference-counted, with [`RefCounted::get_count`]
/// returning a reference to it.
pub struct RefCount {
    count: AtomicUsize,
}

impl RefCount {
    /// Constructs a new instance of [`RefCount`].
    pub fn new() -> Self {
        Self {
            count: AtomicUsize::new(1),
        }
    }
}

impl Default for RefCount {
    fn default() -> Self {
        Self::new()
    }
}
