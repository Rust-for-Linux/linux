// SPDX-License-Identifier: GPL-2.0

//! Rust implementation of `debugfs`.
//!
//! This module allows Rust kernel modules to create directories and files in
//! `/debugfs`.
//!
//! C header: [`include/linux/debugfs.h`](../../../include/linux/debugfs.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/filesystems/debugfs.html>

use alloc::boxed::Box;
use core::{
    any::Any,
    marker::{PhantomData, Sync},
    ptr,
};

use crate::{
    bindings::{self, debugfs_remove_with_callback},
    error,
    file::{OpenAdapter, Operations, OperationsVtable},
    str::CStr,
    types::PointerWrapper,
    Result,
};

/// An `dentry` for a directory in debugfs.
pub struct DebugFsDirectory {
    dentry: *mut bindings::dentry,
    has_parent: bool,
}

// SAFETY: There are no public functions that take a shared [`DebugFsDirectory`]
// reference and all its fields are private so a thread can't actually do
// anything with a `&DebugFsDirectory`. This makes it is safe to share across
// threads.
unsafe impl Sync for DebugFsDirectory {}

impl DebugFsDirectory {
    /// Create a new directory in `debugfs` under `parent`. If `parent` is
    /// `None`, it will be created at the `debugfs` root and removed on drop. If
    /// a `parent` is given then the `parent` is responsible for removing the
    /// directory.
    pub fn create(name: &CStr, parent: Option<&mut DebugFsDirectory>) -> Result<Self> {
        let name = name.as_char_ptr();
        let has_parent = parent.is_some();
        let parent_ptr = parent.map(|p| p.dentry).unwrap_or_else(ptr::null_mut);
        // SAFETY: Calling a C function. `name` is a valid null-terminated
        // string because it came from a [`CStr`] and `parent` is either null or
        // valid because it came from a [`DebugFsDirectory`].
        let dentry =
            error::from_kernel_err_ptr(unsafe { bindings::debugfs_create_dir(name, parent_ptr) })?;
        Ok(DebugFsDirectory { dentry, has_parent })
    }
}

impl Drop for DebugFsDirectory {
    fn drop(&mut self) {
        // If this entry has a parent, we don't need to worry about removal
        // because the parent will remove its children when dropped. Otherwise
        // we need to clean up.
        if !self.has_parent {
            // SAFETY: Calling a C function. `dentry` must have been created by
            // a call to `DebugFsDirectory::create` which always returns a
            // valid `dentry`. There is no parent, so the
            // `dentry` couldn't have been removed and must still be valid.
            //
            // This `dentry` and every `dentry` in it was created with either
            // `DebugFsDirectory::create` or `DebugFsFile::create`. Both
            // functions guarantee that the created `dentry` has a valide
            // `inode` and the `inode`'s `i_private` field will be either null
            // or come from calling `PointerWrapper::into_pointer` on a
            // `Box<Box<dyn Any>>`. They both only create dentry's in debugfs.
            // This makes it safe to call `remove_rust_dentry` on each `dentry`
            // in `self.dentry`.
            unsafe { debugfs_remove_with_callback(self.dentry, Some(remove_rust_dentry)) };
        }
    }
}

/// A `dentry` for a file in debugfs with a `T` stored in `i_private`.
pub struct DebugFsFile<T> {
    dentry: Option<*mut bindings::dentry>,
    _t: PhantomData<T>,
}

// SAFETY: There are no public methods available on [`DebugFsFile`] so a thread
// can't actually do anything with a `&DebugFsFile`. This makes it is safe to
// share across threads.
unsafe impl<T> Sync for DebugFsFile<T> {}

impl<T: Any> DebugFsFile<T> {
    /// Create a file in the `debugfs` directory under `parent`. If `parent` is
    /// `None` then the file will be created at the root of the `debugfs`
    /// directory and it will be removed on drop. If a `parent` is provided then
    /// the `parent` is responsible for removing this file.
    ///
    /// # Safety
    ///
    /// `fops` must be valid when opening an `inode` with a `Box<Box<dyn
    /// Any>>::into_pointer` that can be downcast to `T` stored in `i_private`.
    unsafe fn create(
        name: &CStr,
        parent: Option<&mut DebugFsDirectory>,
        data: T,
        fops: &'static bindings::file_operations,
    ) -> Result<DebugFsFile<T>> {
        let has_parent = parent.is_some();
        let name = name.as_char_ptr();
        let boxed1: Box<dyn Any> = Box::try_new(data)?;
        let boxed2 = Box::try_new(boxed1)?;
        let data = PointerWrapper::into_pointer(boxed2) as *mut _;
        let parent = parent.map(|p| p.dentry).unwrap_or_else(ptr::null_mut);
        // SAFETY: Calling a C function. `name` will be a valid null-terminated
        // string because it came from a [`CStr`]. The caller guarantees that
        // `fops` is valid for an inode with a `Box<Box<dyn Any>>::into_pointer`
        // that can be downcast to `T` stored in `i_private`.
        let dentry_ptr = error::from_kernel_err_ptr(unsafe {
            bindings::debugfs_create_file(name, 0, parent, data, fops)
        });
        match dentry_ptr {
            Err(err) => {
                // SAFETY: `data` was created by calling
                // `PointerWrapper::into_pointer` on a `Box<Box<dyn Any>>` just
                // above.
                let _: Box<Box<dyn Any>> = unsafe { PointerWrapper::from_pointer(data) };
                Err(err)
            }
            Ok(dentry) => Ok(DebugFsFile {
                dentry: if has_parent { None } else { Some(dentry) },
                _t: PhantomData,
            }),
        }
    }
}

impl<T> Drop for DebugFsFile<T> {
    fn drop(&mut self) {
        // If there is no dentry then this file has a parent `DebugFsDirectory`
        // which is responsible for removal.
        if let Some(dentry) = self.dentry {
            // SAFETY: Calling a C function. `dentry` must have been created by
            // a call to [`DebugFsFile::create`] which always returns a valid
            // `dentry`. Since there is no parent that can remove the `dentry`
            // it must still exist.
            //
            // A `DebugFsFile` is created by calling `debugfs_create_file`
            // (which always creates a valid `dentry` in debugfs with a valid
            // `d_inode` field) and passing in a pointer coming from a
            // `Box<Box<dyn Any>>` which gets put in the `inode`'s `i_private`
            // field. This is sufficient for `remove_rust_dentry` to be safely
            // called on the `dentry`.
            unsafe { debugfs_remove_with_callback(dentry, Some(remove_rust_dentry)) };
        }
    }
}

/// # Safety
/// `dentry` must be a valid `bindings::dentry` with a valid `d_inode` field. In
/// addition, the `i_private` field of `d_inode` must be either a null pointer
/// or one created by calling `PointerWrapper::into_pointer` on a `Box<Box<dyn
/// Any>>`.
unsafe extern "C" fn drop_i_private(dentry: *mut bindings::dentry) {
    // SAFETY: Caller guarantees that `dentry->d_inode` can be dereferenced.
    let i_private = unsafe { (*(*dentry).d_inode).i_private };
    // SAFETY: Caller guarantees that `dentry->d_inode->i_private` is either
    // null, or generated by calling `PointerWrapper::into_pointer` on a
    // `Box<Box<dyn Any>>`.
    if !i_private.is_null() {
        let _: Box<Box<dyn Any>> = unsafe { PointerWrapper::from_pointer(i_private) };
    }
}

/// # Safety
/// `dentry` must be in debugfs and satify all the requirements for
/// `drop_i_private` to be safely called on it.
unsafe extern "C" fn remove_rust_dentry(dentry: *mut bindings::dentry) {
    // SAFETY: The caller is responsible for ensuring that `drop_i_private` can
    // be called on `dentry` and that the dentry is in debugfs.
    unsafe { bindings::debugfs_remove_one_with_callback(dentry, Some(drop_i_private)) }
}

impl<T: Any + Sync> OpenAdapter<T> for DebugFsFile<T> {
    unsafe fn convert(inode: *mut bindings::inode, _file: *mut bindings::file) -> *const T {
        let data: &dyn Any =
            unsafe { <Box<Box<dyn Any>> as PointerWrapper>::borrow((*inode).i_private) }.as_ref();
        let open_data: &T = match data.downcast_ref() {
            Some(data) => data,
            None => panic!(),
        };
        open_data
    }
}

/// Create a file in `debugfs` under `parent`. If `parent` is `None` then the
/// folder will be created at the top level of `debugfs`.
pub fn debugfs_create<T: Operations>(
    name: &CStr,
    parent: Option<&mut DebugFsDirectory>,
    data: T::OpenData,
) -> Result<DebugFsFile<T::OpenData>>
where
    T::OpenData: 'static,
{
    // SAFETY: The `OpenAdapter` implementation for `DebugFsFile<T::OpenData>`
    // expects a the opened `inode` to have a `Box<Box<dyn Any>>` in `i_private`
    // that can be downcast to `T::OpenData`. `data` has type `T::OpenData` so
    // this satisfies the safety requirements on `DebugFsFile::create`.
    unsafe {
        DebugFsFile::create(
            name,
            parent,
            data,
            OperationsVtable::<DebugFsFile<T::OpenData>, T>::build(),
        )
    }
}
