use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::{
    error::Result,
    fs::FileSystem,
    pr_info,
    prelude::EDOM,
    types::{ARef, AlwaysRefCounted, Lockable, Locked, Opaque},
};

/// The type of a [`Folio`] is unspecified.
pub struct Unspecified;

/// The [`Folio`] instance is a page-cache one.
pub struct PageCache<T: FileSystem + ?Sized>(PhantomData<T>);

/// A folio.
///
/// The `S` type parameter specifies the type of folio.
///
/// Wraps the kernel's `struct folio`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `folio_get` ensures that the
/// allocation remains valid at least until the matching call to `folio_put`.
#[repr(transparent)]
pub struct Folio<S = Unspecified>(pub(crate) Opaque<bindings::folio>, PhantomData<S>);

// SAFETY: The type invariants guarantee that `Folio` is always ref-counted.
unsafe impl<S> AlwaysRefCounted for Folio<S> {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe {
            bindings::folio_get(self.0.get());
        }
    }

    unsafe fn dec_ref(obj: ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe {
            bindings::folio_put(obj.as_ref().0.get());
        }
    }
}

impl<S> Folio<S> {
    /// Creates a new folio reference from the given raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    /// * `ptr` is valid and remains so for the lifetime of the returned reference.
    /// * The folio has the right state.
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::folio) -> &'a Self {
        // SAFETY: The safety requirements guarantee that the cast below is ok.
        unsafe { &*ptr.cast() }
    }

    /// Returns the byte size of this folio.
    pub fn size(&self) -> usize {
        // SAFETY: The folio is valid because the shared reference implies a non-zero refcount.
        unsafe { bindings::folio_size(self.0.get()) }
    }

    /// Returns true if the folio is in highmem.
    pub fn test_highmem(&self) -> bool {
        // SAFETY: The folio is valid because the shared reference implies a non-zero refcount.
        unsafe { bindings::folio_test_highmem(self.0.get()) }
    }

    /// Consumes the folio and returns an owned mapped reference.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the folio is not concurrently mapped for write.
    pub unsafe fn map_owned(folio: ARef<Self>, offset: usize) -> Result<Mapped<'static, S>> {
        // SAFETY: The safety requirements of this function satisfy those of `map`.
        let guard = unsafe { folio.map(offset)? };
        let to_unmap = guard.page;
        let data = &guard[0] as *const u8;
        let data_len = guard.len();
        core::mem::forget(guard);
        Ok(Mapped {
            _folio: folio,
            to_unmap,
            data,
            data_len,
            _p: PhantomData,
        })
    }

    /// Maps the contents of a folio page into a slice.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the folio is not concurrently mapped for write.
    pub unsafe fn map(&self, offset: usize) -> Result<MapGuard<'_>> {
        if offset > self.size() {
            return Err(EDOM);
        }

        let page_index = offset / bindings::PAGE_SIZE;
        let page_offset = offset % bindings::PAGE_SIZE;

        // SAFETY: We just checked that the index is within bounds of the folio.
        let page = unsafe { bindings::folio_page(self.0.get(), page_index) };

        // SAFETY: `page` is valid because it was returned by `folio_page` above.
        let ptr = unsafe { bindings::kmap(page) };

        let size = if self.test_highmem() {
            bindings::PAGE_SIZE
        } else {
            self.size()
        };

        // SAFETY: We just mapped `ptr`, so it's valid for read.
        let data = unsafe {
            core::slice::from_raw_parts(ptr.cast::<u8>().add(page_offset), size - page_offset)
        };
        Ok(MapGuard { data, page })
    }
}

/// An owned mapped folio.
///
/// That is, a mapped version of a folio that holds a reference to it.
///
/// The lifetime is used to tie the mapping to other lifetime, for example, the lifetime of a lock
/// guard. This allows the mapping to exist only while a lock is held.
///
/// # Invariants
///
/// `to_unmap` is a mapped page of the folio. The byte range starting at `data` and extending for
/// `data_len` bytes is within the mapped page.
pub struct Mapped<'a, S = Unspecified> {
    _folio: ARef<Folio<S>>,
    to_unmap: *mut bindings::page,
    data: *const u8,
    data_len: usize,
    _p: PhantomData<&'a ()>,
}

impl<S> Mapped<'_, S> {
    pub fn cap_len(&mut self, new_len: usize) {
        if new_len < self.data_len {
            self.data_len = new_len;
        }
    }
}

impl<S> Deref for Mapped<'_, S> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { core::slice::from_raw_parts(self.data, self.data_len) }
    }
}

impl<S> Drop for Mapped<'_, S> {
    fn drop(&mut self) {
        // SAFETY: By the type invariant, we know that `to_unmap` is mapped.
        unsafe { bindings::kunmap(self.to_unmap) };
    }
}

/// A mapped [`Folio`].
pub struct MapGuard<'a> {
    data: &'a [u8],
    page: *mut bindings::page,
}

impl Deref for MapGuard<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl Drop for MapGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: A `MapGuard` instance is only created when `kmap` succeeds, so it's ok to unmap
        // it when the guard is dropped.
        unsafe { bindings::kunmap(self.page) };
    }
}

/// A mapped mutable [`Folio`].
pub struct MapGuardMut<'a> {
    data: &'a mut [u8],
    page: *mut bindings::page,
}

impl Deref for MapGuardMut<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl DerefMut for MapGuardMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl Drop for MapGuardMut<'_> {
    fn drop(&mut self) {
        pr_info!("Dropped guard, set pag dirty and unmapped\n");
        // SAFETY: A `MapGuard` instance is only created when `kmap` succeeds, so it's ok to unmap
        // it when the guard is dropped.
        unsafe {
            bindings::set_page_dirty(self.page);
            bindings::kunmap(self.page)
        };
    }
}

// SAFETY: `raw_lock` calls folio_lock, which actually locks the folio.
unsafe impl<S> Lockable for Folio<S> {
    fn raw_lock(&self) {
        pr_info!("Locked folio\n");
        // SAFETY: The folio is valid because the shared reference implies a non-zero refcount.
        unsafe { bindings::folio_lock(self.0.get()) }
    }

    unsafe fn unlock(&self) {
        pr_info!("unlocked folio\n");
        // SAFETY: The safety requirements guarantee that the folio is locked.
        unsafe { bindings::folio_unlock(self.0.get()) }
    }
}

impl<T: Deref<Target = Folio<S>>, S> Locked<T> {
    /// SAFETY: it is guarenteed that the folio is locked by the type invariant
    pub fn map(&self, offset: usize) -> Result<MapGuardMut<'_>> {
        if offset > self.size() {
            return Err(EDOM);
        }

        let page_index = offset / bindings::PAGE_SIZE;
        let page_offset = offset % bindings::PAGE_SIZE;

        // SAFETY: We just checked that the index is within bounds of the folio.
        let page = unsafe { bindings::folio_page(self.0.get(), page_index) };

        // SAFETY: `page` is valid because it was returned by `folio_page` above.
        let ptr = unsafe { bindings::kmap(page) };

        let size = if self.test_highmem() {
            bindings::PAGE_SIZE
        } else {
            self.size()
        };

        // SAFETY: We just mapped `ptr`, so it's valid for read.
        let data = unsafe {
            core::slice::from_raw_parts_mut(ptr.cast::<u8>().add(page_offset), size - page_offset)
        };

        Ok(MapGuardMut { data, page })
    }
}
