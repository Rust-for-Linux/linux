#![allow(missing_docs)]
// SPDX-License-Identifier: GPL-2.0

//! API to add files to debugfs.
//!
//! C header: [`include/linux/debugfs.h`](../../../../include/linux/debugfs.h)
//!
//! Reference: <https://www.kernel.org/doc/html/next/filesystems/debugfs.html>

use crate::error::{from_err_ptr, Result};
use crate::file;
use crate::prelude::*;
use crate::str::CStr;
use crate::sync::Arc;
use crate::types::Mode;
use core::ffi::c_void;

pub type PinnedRegistration<T = ()> = Pin<Box<Registration<T>>>;

/// A registration of a debugfs directory or file
pub struct Registration<T> {
    open_data: T,
    dentry: *mut bindings::dentry,
    _parent: Option<Arc<Registration<()>>>, // Store parent to prevent it from being dropped
}

// SAFETY: dentry is only being held by the struct and is not shared with anyone else, so if T is
// Send, it is safe to send this struct to another thread.
unsafe impl<T: Send> Send for Registration<T> {}

// SAFETY: dentry is never accessed except in Registration::drop. As long as T is Sync, then
// it is safe for Registration to be Sync
unsafe impl<T: Sync> Sync for Registration<T> {}

impl<T> Drop for Registration<T> {
    fn drop(&mut self) {
        // SAFETY: self.dentry is valid by the type invariant.
        unsafe {
            bindings::debugfs_remove(self.dentry);
        }
    }
}

impl Registration<()> {
    pub fn register_symlink(
        name: &'static CStr,
        parent: Option<Arc<Registration<()>>>,
        dest: &'static CStr,
    ) -> Result<Registration<()>> {
        let parent_dentry = parent.as_ref().map_or(core::ptr::null_mut(), |r| r.dentry);

        let dentry = from_err_ptr(unsafe {
            bindings::debugfs_create_symlink(name.as_char_ptr(), parent_dentry, dest.as_char_ptr())
        })?;

        Ok(Self {
            dentry,
            open_data: (),
            _parent: parent,
        })
    }

    pub fn register_dir(
        name: &'static CStr,
        parent: Option<Arc<Registration<()>>>,
    ) -> Result<Registration<()>> {
        let parent_dentry = parent.as_ref().map_or(core::ptr::null_mut(), |r| r.dentry);

        // SAFETY: name.as_char_ptr() cannot be null. The type invariant ensure that
        // self.dentry is always a valid pointer, so p will always be a NULL pointer or a valid
        // pointer.
        let dentry = from_err_ptr(unsafe {
            bindings::debugfs_create_dir(name.as_char_ptr(), parent_dentry)
        })?;

        Ok(Self {
            dentry,
            open_data: (),
            _parent: parent,
        })
    }
}

impl<T: Sync> Registration<T> {
    pub fn register_file<U>(
        name: &'static CStr,
        mode: Mode,
        open_data: T,
        parent: Option<Arc<Registration<()>>>,
    ) -> Result<PinnedRegistration<T>>
    where
        Self: file::OpenAdapter<T>,
        U: file::Operations<OpenData = T>,
    {
        let fops = unsafe { file::OperationsVtable::<Self, U>::build() };
        let parent_dentry = parent.as_ref().map_or(core::ptr::null_mut(), |r| r.dentry);

        let mut registration = Pin::from(Box::try_new(Self {
            dentry: core::ptr::null_mut(),
            open_data,
            _parent: parent,
        })?);
        // SAFETY: The function never moves `this` hence the call is safe.
        let this = unsafe { registration.as_mut().get_unchecked_mut() };
        this.dentry = from_err_ptr(unsafe {
            bindings::debugfs_create_file(
                name.as_char_ptr(),
                mode.as_int(),
                parent_dentry,
                this as *mut _ as *mut c_void,
                fops,
            )
        })?;

        Ok(registration)
    }
}

impl<T: Sync> file::OpenAdapter<T> for Registration<T> {
    // WIP: Returns a valid pointer that lives longer than the call to the open function
    unsafe fn convert(inode: *mut bindings::inode, _file: *mut bindings::file) -> *const T {
        // SAFETY: debugfs_create_file is called with self as private data. The C debugfs API
        // stores it into the inode.i_private field.
        let this: &Self = unsafe { &*((*inode).i_private as *const Self) };
        &this.open_data
    }
}

pub mod attr {
    use super::Registration;
    use crate::error::Result;
    use crate::file;
    use crate::prelude::*;
    use crate::str::CStr;
    use crate::sync::{Arc, ArcBorrow};
    use core::ffi::{c_int, c_void};

    pub trait Attribute<T> {
        fn get(&self) -> Result<T>;
        fn set(&self, val: T) -> Result;
    }

    pub struct AttributeData {
        private_data: *mut c_void,
    }

    unsafe impl Sync for AttributeData {}
    unsafe impl Send for AttributeData {}

    pub fn open(
        file: &file::File,
        get: Option<unsafe extern "C" fn(_: *mut c_void, _: *mut u64) -> c_int>,
        set: Option<unsafe extern "C" fn(_: *mut c_void, _: u64) -> c_int>,
        fmt: &CStr,
    ) -> Result<Arc<AttributeData>> {
        let file = file.as_ptr();

        let private_data = unsafe {
            bindings::simple_attr_open((*file).f_inode, file, get, set, fmt.as_char_ptr());
            (*file).private_data
        };

        Ok(Arc::try_new(AttributeData { private_data })?)
    }

    pub fn release(_data: Arc<AttributeData>, file: &file::File) {
        let file = file.as_ptr();
        unsafe { bindings::simple_attr_release((*file).f_inode, file) };
    }

    pub fn read(
        data: ArcBorrow<'_, AttributeData>,
        file: &file::File,
        writer: &mut impl crate::io_buffer::IoBufferWriter,
        offset: u64,
    ) -> Result<usize> {
        let mut ppos = offset as bindings::loff_t;
        let file = file.as_ptr();
        let buf = writer.buffer().unwrap() as *mut i8;

        let ret = unsafe {
            let private_data = (*file).private_data;
            (*file).private_data = data.private_data;
            let ret = bindings::debugfs_attr_read(file, buf, writer.len(), &mut ppos);
            (*file).private_data = private_data;
            ret
        };

        if ret < 0 {
            Err(Error::from_errno(ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    pub fn write(
        data: ArcBorrow<'_, AttributeData>,
        file: &file::File,
        reader: &mut impl crate::io_buffer::IoBufferReader,
        offset: u64,
        signed: bool,
    ) -> Result<usize> {
        let mut ppos = offset as bindings::loff_t;
        let file = file.as_ptr();
        let buf = reader.buffer().unwrap() as *mut i8;

        let ret = unsafe {
            let private_data = (*file).private_data;
            (*file).private_data = data.private_data;
            let ret = match signed {
                true => bindings::debugfs_attr_write_signed(file, buf, reader.len(), &mut ppos),
                false => bindings::debugfs_attr_write(file, buf, reader.len(), &mut ppos),
            };
            (*file).private_data = private_data;
            ret
        };

        if ret < 0 {
            Err(Error::from_errno(ret as i32))
        } else {
            Ok(ret as usize)
        }
    }

    pub extern "C" fn _get_callback_unsigned<T: Attribute<u64>>(
        this: *mut core::ffi::c_void,
        val: *mut u64,
    ) -> core::ffi::c_int {
        let this: &Registration<Arc<T>> = unsafe { &mut *(this as *mut _) };
        match this.open_data.get() {
            Ok(v) => {
                unsafe { *val = v };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    pub extern "C" fn _get_callback_signed<T: Attribute<i64>>(
        this: *mut core::ffi::c_void,
        val: *mut u64,
    ) -> core::ffi::c_int {
        let this: &Registration<Arc<T>> = unsafe { &mut *(this as *mut _) };
        match this.open_data.get() {
            Ok(v) => {
                unsafe { *val = v as u64 };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    pub extern "C" fn _set_callback_unsigned<T: Attribute<u64>>(
        this: *mut core::ffi::c_void,
        val: u64,
    ) -> core::ffi::c_int {
        let this: &Registration<Arc<T>> = unsafe { &mut *(this as *mut _) };
        match this.open_data.set(val) {
            Ok(_) => 0,
            Err(e) => e.to_errno(),
        }
    }

    pub extern "C" fn _set_callback_signed<T: Attribute<i64>>(
        this: *mut core::ffi::c_void,
        val: u64,
    ) -> core::ffi::c_int {
        let this: &Registration<Arc<T>> = unsafe { &mut *(this as *mut _) };
        match this.open_data.set(val as i64) {
            Ok(_) => 0,
            Err(e) => e.to_errno(),
        }
    }
}

#[macro_export]
macro_rules! attribute {
    ($attribute_type:ty, $fmt:literal, $is_signed:literal, $getter:expr, $setter:expr) => {
        impl $attribute_type {
            fn register(
                self: $crate::sync::Arc<Self>,
                name: &'static $crate::str::CStr,
                mode: $crate::types::Mode,
                parent: ::core::option::Option<
                    $crate::sync::Arc<$crate::debugfs::Registration<()>>,
                >,
            ) -> $crate::error::Result<$crate::debugfs::PinnedRegistration<$crate::sync::Arc<Self>>>
            {
                $crate::debugfs::Registration::<$crate::sync::Arc<Self>>::register_file::<Self>(
                    name, mode, self, parent,
                )
            }
        }

        #[vtable]
        impl $crate::file::Operations for $attribute_type {
            type OpenData = $crate::sync::Arc<Self>;
            type Data = $crate::sync::Arc<$crate::debugfs::attr::AttributeData>;

            fn open(
                data: &Self::OpenData,
                file: &$crate::file::File,
            ) -> $crate::error::Result<Self::Data> {
                use ::core::option::Option::Some;

                $crate::debugfs::attr::open(
                    file,
                    Some($getter),
                    Some($setter),
                    $crate::c_str!($fmt),
                )
            }

            fn release(data: Self::Data, file: &$crate::file::File) {
                $crate::debugfs::attr::release(data, file);
            }

            fn read(
                data: $crate::sync::ArcBorrow<'_, $crate::debugfs::attr::AttributeData>,
                file: &$crate::file::File,
                writer: &mut impl $crate::io_buffer::IoBufferWriter,
                offset: u64,
            ) -> $crate::error::Result<usize> {
                $crate::debugfs::attr::read(data, file, writer, offset)
            }

            fn write(
                data: $crate::sync::ArcBorrow<'_, $crate::debugfs::attr::AttributeData>,
                file: &$crate::file::File,
                reader: &mut impl $crate::io_buffer::IoBufferReader,
                offset: u64,
            ) -> $crate::error::Result<usize> {
                $crate::debugfs::attr::write(data, file, reader, offset, $is_signed)
            }
        }
    };
}

#[macro_export]
macro_rules! attribute_unsigned {
    ($attribute_type:ty, $fmt:literal) => {
        $crate::debugfs::attribute!(
            $attribute_type,
            $fmt,
            false,
            $crate::debugfs::attr::_get_callback_unsigned::<Self>,
            $crate::debugfs::attr::_set_callback_unsigned::<Self>
        );
    };
}

#[macro_export]
macro_rules! attribute_signed {
    ($attribute_type:ty, $fmt:literal) => {
        $crate::debugfs::attribute!(
            $attribute_type,
            $fmt,
            true,
            $crate::debugfs::attr::_get_callback_signed::<Self>,
            $crate::debugfs::attr::_set_callback_signed::<Self>
        );
    };
}

pub use attribute;
pub use attribute_signed;
pub use attribute_unsigned;
