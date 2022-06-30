// SPDX-License-Identifier: GPL-2.0

//! System control.
//!
//! C header: [`include/linux/sysctl.h`](../../../../include/linux/sysctl.h)
//!
//! Reference: <https://www.kernel.org/doc/Documentation/sysctl/README>

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;
use core::ptr;
use core::sync::atomic;

use crate::{
    bindings,
    error::code::*,
    io_buffer::IoBufferWriter,
    str::CStr,
    types,
    user_ptr::{UserSlicePtr, UserSlicePtrWriter},
    Result,
};

/// Sysctl storage.
pub trait SysctlStorage: Sync {
    /// Writes a byte slice.
    fn store_value(&self, data: &[u8]) -> (usize, Result);

    /// Reads via a [`UserSlicePtrWriter`].
    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, Result);
}

fn trim_whitespace(mut data: &[u8]) -> &[u8] {
    while !data.is_empty() && (data[0] == b' ' || data[0] == b'\t' || data[0] == b'\n') {
        data = &data[1..];
    }
    while !data.is_empty()
        && (data[data.len() - 1] == b' '
            || data[data.len() - 1] == b'\t'
            || data[data.len() - 1] == b'\n')
    {
        data = &data[..data.len() - 1];
    }
    data
}

impl<T> SysctlStorage for &T
where
    T: SysctlStorage,
{
    fn store_value(&self, data: &[u8]) -> (usize, Result) {
        (*self).store_value(data)
    }

    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, Result) {
        (*self).read_value(data)
    }
}

impl SysctlStorage for atomic::AtomicBool {
    fn store_value(&self, data: &[u8]) -> (usize, Result) {
        let result = match trim_whitespace(data) {
            b"0" => {
                self.store(false, atomic::Ordering::Relaxed);
                Ok(())
            }
            b"1" => {
                self.store(true, atomic::Ordering::Relaxed);
                Ok(())
            }
            _ => Err(EINVAL),
        };
        (data.len(), result)
    }

    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, Result) {
        let value = if self.load(atomic::Ordering::Relaxed) {
            b"1\n"
        } else {
            b"0\n"
        };
        (value.len(), data.write_slice(value))
    }
}

/// Holds a single `sysctl` entry (and its table).
pub struct Sysctl<T: SysctlStorage> {
    inner: Box<T>,
    // Responsible for keeping the `ctl_table` alive.
    _table: Box<[bindings::ctl_table]>,
    header: *mut bindings::ctl_table_header,
}

// SAFETY: The only public method we have is `get()`, which returns `&T`, and
// `T: Sync`. Any new methods must adhere to this requirement.
unsafe impl<T: SysctlStorage> Sync for Sysctl<T> {}

unsafe extern "C" fn proc_handler<T: SysctlStorage>(
    ctl: *mut bindings::ctl_table,
    write: core::ffi::c_int,
    buffer: *mut core::ffi::c_void,
    len: *mut usize,
    ppos: *mut bindings::loff_t,
) -> core::ffi::c_int {
    // If we are reading from some offset other than the beginning of the file,
    // return an empty read to signal EOF.
    if unsafe { *ppos } != 0 && write == 0 {
        unsafe { *len = 0 };
        return 0;
    }

    let data = unsafe { UserSlicePtr::new(buffer, *len) };
    let storage = unsafe { &*((*ctl).data as *const T) };
    let (bytes_processed, result) = if write != 0 {
        let data = match data.read_all() {
            Ok(r) => r,
            Err(e) => return e.to_kernel_errno(),
        };
        storage.store_value(&data)
    } else {
        let mut writer = data.writer();
        storage.read_value(&mut writer)
    };
    unsafe { *len = bytes_processed };
    unsafe { *ppos += *len as bindings::loff_t };
    match result {
        Ok(()) => 0,
        Err(e) => e.to_kernel_errno(),
    }
}

impl<T: SysctlStorage> Sysctl<T> {
    /// Registers a single entry in `sysctl`.
    pub fn register(
        path: &'static CStr,
        name: &'static CStr,
        storage: T,
        mode: types::Mode,
    ) -> Result<Sysctl<T>> {
        if name.contains(&b'/') {
            return Err(EINVAL);
        }

        let storage = Box::try_new(storage)?;
        let mut table = Vec::try_with_capacity(2)?;
        table.try_push(bindings::ctl_table {
            procname: name.as_char_ptr(),
            mode: mode.as_int(),
            data: &*storage as *const T as *mut core::ffi::c_void,
            proc_handler: Some(proc_handler::<T>),

            maxlen: 0,
            child: ptr::null_mut(),
            poll: ptr::null_mut(),
            extra1: ptr::null_mut(),
            extra2: ptr::null_mut(),
        })?;
        table.try_push(unsafe { mem::zeroed() })?;
        let mut table = table.try_into_boxed_slice()?;

        let result = unsafe { bindings::register_sysctl(path.as_char_ptr(), table.as_mut_ptr()) };
        if result.is_null() {
            return Err(ENOMEM);
        }

        Ok(Sysctl {
            inner: storage,
            _table: table,
            header: result,
        })
    }

    /// Gets the storage.
    pub fn get(&self) -> &T {
        &self.inner
    }
}

impl<T: SysctlStorage> Drop for Sysctl<T> {
    fn drop(&mut self) {
        unsafe {
            bindings::unregister_sysctl_table(self.header);
        }
        self.header = ptr::null_mut();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_whitespace() {
        assert_eq!(trim_whitespace(b"foo    "), b"foo");
        assert_eq!(trim_whitespace(b"    foo"), b"foo");
        assert_eq!(trim_whitespace(b"  foo  "), b"foo");
    }
}
