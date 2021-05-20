// SPDX-License-Identifier: GPL-2.0

//! System control.
//!
//! C header: [`include/linux/sysctl.h`](../../../../include/linux/sysctl.h)
//!
//! Reference: <https://www.kernel.org/doc/Documentation/sysctl/README>

use alloc::boxed::Box;
use alloc::vec;
use core::mem;
use core::ptr::{self, NonNull};
use core::sync::atomic;

use crate::{
    bindings, c_types, error,
    io_buffer::IoBufferWriter,
    types,
    user_ptr::{UserSlicePtr, UserSlicePtrWriter},
};

/// Sysctl storage.
pub trait SysctlStorage: Sync {
    /// Writes a byte slice.
    fn store_value(&self, data: &[u8]) -> (usize, error::Result);

    /// Reads via a [`UserSlicePtrWriter`].
    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, error::Result);
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
    fn store_value(&self, data: &[u8]) -> (usize, error::Result) {
        (*self).store_value(data)
    }

    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, error::Result) {
        (*self).read_value(data)
    }
}

impl SysctlStorage for atomic::AtomicBool {
    fn store_value(&self, data: &[u8]) -> (usize, error::Result) {
        let result = match trim_whitespace(data) {
            b"0" => {
                self.store(false, atomic::Ordering::Relaxed);
                Ok(())
            }
            b"1" => {
                self.store(true, atomic::Ordering::Relaxed);
                Ok(())
            }
            _ => Err(error::Error::EINVAL),
        };
        (data.len(), result)
    }

    fn read_value(&self, data: &mut UserSlicePtrWriter) -> (usize, error::Result) {
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
    header: NonNull<bindings::ctl_table_header>,
}

// SAFETY: The only public method we have is `get()`, which returns `&T`, and
// `T: Sync`. Any new methods must adhere to this requirement.
unsafe impl<T: SysctlStorage> Sync for Sysctl<T> {}

unsafe extern "C" fn proc_handler<T: SysctlStorage>(
    ctl: *mut bindings::ctl_table,
    write: c_types::c_int,
    buffer: *mut c_types::c_void,
    len: *mut usize,
    ppos: *mut bindings::loff_t,
) -> c_types::c_int {
    // If we are reading from some offset other than the beginning of the file,
    // return an empty read to signal EOF.
    if *ppos != 0 && write == 0 {
        *len = 0;
        return 0;
    }

    let data = UserSlicePtr::new(buffer, *len);
    let storage = &*((*ctl).data as *const T);
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
    *len = bytes_processed;
    *ppos += *len as bindings::loff_t;
    match result {
        Ok(()) => 0,
        Err(e) => e.to_kernel_errno(),
    }
}

impl<T: SysctlStorage> Sysctl<T> {
    /// Registers a single entry in `sysctl`.
    pub fn register(
        path: types::CStr<'static>,
        name: types::CStr<'static>,
        storage: T,
        mode: types::Mode,
    ) -> error::Result<Sysctl<T>> {
        if name.contains('/') {
            return Err(error::Error::EINVAL);
        }

        let storage = Box::try_new(storage)?;
        let mut table = vec![
            bindings::ctl_table {
                procname: name.as_ptr() as *const i8,
                mode: mode.as_int(),
                data: &*storage as *const T as *mut c_types::c_void,
                proc_handler: Some(proc_handler::<T>),

                maxlen: 0,
                child: ptr::null_mut(),
                poll: ptr::null_mut(),
                extra1: ptr::null_mut(),
                extra2: ptr::null_mut(),
            },
            unsafe { mem::zeroed() },
        ]
        .into_boxed_slice();

        let result =
            unsafe { bindings::register_sysctl(path.as_ptr() as *const i8, table.as_mut_ptr()) };

        let header = NonNull::new(result).ok_or(error::Error::ENOMEM)?;

        Ok(Sysctl {
            inner: storage,
            _table: table,
            header,
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
            bindings::unregister_sysctl_table(self.header.as_ptr());
        }
        // TODO: this previously set `self.header` to `NULL` here without an apparent reason why.
        // `self.header` should not be accessible after droping it.
    }
}
