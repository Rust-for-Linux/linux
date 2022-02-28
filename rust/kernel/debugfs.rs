// SPDX-License-Identifier: GPL-2.0

//! TODOABK: finish doc

use core::{
    marker::{PhantomData, Sync},
    ptr,
};

use crate::{bindings, c_types, error, str::CStr, types::PointerWrapper, Result};

/// An `inode` in debugfs with a `T` stored in `i_private`.
pub(crate) struct DebugFsDirEntry<T: PointerWrapper> {
    dentry: *mut bindings::dentry,
    data: *mut c_types::c_void,
    _wrapper: PhantomData<T>,
}

// SAFETY: There are methods available on [`DebugFsDirEntry`] so a thread can't
// actually do anything with a `&DebugFsDirEntry`. This makes it is safe to
// share across threads.
unsafe impl<T: PointerWrapper> Sync for DebugFsDirEntry<T> {}

impl<T: PointerWrapper> DebugFsDirEntry<T> {
    /// Create a file in `debugfs`.
    ///
    /// # Safety
    ///
    /// `fops` must be valid when opening an `inode` with `data::into_pointer`
    /// stored in `i_private`.
    pub(crate) unsafe fn create_file(
        name: &CStr,
        data: T,
        fops: &'static bindings::file_operations,
    ) -> Result<Self> {
        let name = name.as_char_ptr();
        let data = data.into_pointer() as *mut _;
        // SAFETY: Calling a C function. `name` will be a valid null-terminated
        // string because it came from a `CStr`. The caller guarantees that
        // `fops` is valid for an inode with `data` in `i_private`.
        let dentry_ptr = error::from_kernel_err_ptr(unsafe {
            bindings::debugfs_create_file(name, 0, ptr::null_mut(), data, fops)
        });
        match dentry_ptr {
            Err(err) => {
                // SAFETY: `data` was created by `T::into_pointer` just above.
                drop(unsafe { T::from_pointer(data) });
                Err(err)
            }
            Ok(dentry) => Ok(DebugFsDirEntry {
                dentry,
                data,
                _wrapper: PhantomData,
            }),
        }
    }
}

impl<T: PointerWrapper> Drop for DebugFsDirEntry<T> {
    fn drop(&mut self) {
        // SAFETY: Calling a C function. `dentry` must have been created by a
        // call to [`DebugFsDirEntry::create_file`] which always returns a valid
        // `dentry`.
        unsafe {
            bindings::debugfs_remove(self.dentry);
        }
        // SAFETY: `self.data` was created by a call to `T::into_pointer` in
        // [`DebugFsDirEntry::create_file`].
        unsafe { T::from_pointer(self.data) };
    }
}
