// SPDX-License-Identifier: GPL-2.0-only OR MIT

//! Support for Apple RTKit coprocessors.
//!
//! C header: [`include/linux/soc/apple/rtkit.h`](../../../../include/linux/gpio/driver.h)

use crate::{
    bindings, device,
    error::{code::*, from_kernel_err_ptr, to_result, Error, Result},
    str::CStr,
    types::{ForeignOwnable, ScopeGuard},
};

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ptr;
use macros::vtable;

/// Trait to represent allocatable buffers for the RTKit core.
///
/// Users must implement this trait for their own representation of those allocations.
pub trait Buffer {
    /// Returns the IOVA (virtual address) of the buffer from RTKit's point of view, or an error if
    /// unavailable.
    fn iova(&self) -> Result<usize>;

    /// Returns a mutable byte slice of the buffer contents, or an
    /// error if unavailable.
    fn buf(&mut self) -> Result<&mut [u8]>;
}

/// Callback operations for an RTKit client.
#[vtable]
pub trait Operations {
    /// Arbitrary user context type.
    type Data: ForeignOwnable + Send + Sync;

    /// Type representing an allocated buffer for RTKit.
    type Buffer: Buffer;

    /// Called when RTKit crashes.
    fn crashed(_data: <Self::Data as ForeignOwnable>::Borrowed<'_>) {}

    /// Called when a message was received on a non-system endpoint. Called in non-IRQ context.
    fn recv_message(
        _data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        _endpoint: u8,
        _message: u64,
    ) {
    }

    /// Called in IRQ context when a message was received on a non-system endpoint.
    ///
    /// Must return `true` if the message is handled, or `false` to process it in
    /// the handling thread.
    fn recv_message_early(
        _data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        _endpoint: u8,
        _message: u64,
    ) -> bool {
        false
    }

    /// Allocate a buffer for use by RTKit.
    fn shmem_alloc(
        _data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        _size: usize,
    ) -> Result<Self::Buffer> {
        Err(EINVAL)
    }

    /// Map an existing buffer used by RTKit at a device-specified virtual address.
    fn shmem_map(
        _data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        _iova: usize,
        _size: usize,
    ) -> Result<Self::Buffer> {
        Err(EINVAL)
    }
}

/// Represents `struct apple_rtkit *`.
///
/// # Invariants
///
/// The rtk pointer is valid.
/// The data pointer is a valid pointer from T::Data::into_foreign().
pub struct RtKit<T: Operations> {
    rtk: *mut bindings::apple_rtkit,
    data: *mut core::ffi::c_void,
    _p: PhantomData<T>,
}

unsafe extern "C" fn crashed_callback<T: Operations>(cookie: *mut core::ffi::c_void) {
    T::crashed(unsafe { T::Data::borrow(cookie) });
}

unsafe extern "C" fn recv_message_callback<T: Operations>(
    cookie: *mut core::ffi::c_void,
    endpoint: u8,
    message: u64,
) {
    T::recv_message(unsafe { T::Data::borrow(cookie) }, endpoint, message);
}

unsafe extern "C" fn recv_message_early_callback<T: Operations>(
    cookie: *mut core::ffi::c_void,
    endpoint: u8,
    message: u64,
) -> bool {
    T::recv_message_early(unsafe { T::Data::borrow(cookie) }, endpoint, message)
}

unsafe extern "C" fn shmem_setup_callback<T: Operations>(
    cookie: *mut core::ffi::c_void,
    bfr: *mut bindings::apple_rtkit_shmem,
) -> core::ffi::c_int {
    // SAFETY: `bfr` is a valid buffer
    let bfr_mut = unsafe { &mut *bfr };

    let buf = if bfr_mut.iova != 0 {
        bfr_mut.is_mapped = true;
        T::shmem_map(
            // SAFETY: `cookie` came from a previous call to `into_foreign`.
            unsafe { T::Data::borrow(cookie) },
            bfr_mut.iova as usize,
            bfr_mut.size,
        )
    } else {
        bfr_mut.is_mapped = false;
        // SAFETY: `cookie` came from a previous call to `into_foreign`.
        T::shmem_alloc(unsafe { T::Data::borrow(cookie) }, bfr_mut.size)
    };

    let mut buf = match buf {
        Err(e) => {
            return e.to_kernel_errno();
        }
        Ok(buf) => buf,
    };

    let iova = match buf.iova() {
        Err(e) => {
            return e.to_kernel_errno();
        }
        Ok(iova) => iova,
    };

    let slice = match buf.buf() {
        Err(e) => {
            return e.to_kernel_errno();
        }
        Ok(slice) => slice,
    };

    if slice.len() < bfr_mut.size {
        return ENOMEM.to_kernel_errno();
    }

    bfr_mut.iova = iova as u64;
    bfr_mut.buffer = slice.as_mut_ptr() as *mut _;

    // Now box the returned buffer type and stash it in the private pointer of the
    // `apple_rtkit_shmem` struct for safekeeping.
    match Box::try_new(buf) {
        Err(e) => Error::from(e).to_kernel_errno(),
        Ok(boxed) => {
            bfr_mut.private = Box::into_raw(boxed) as *mut _;
            0
        }
    }
}

unsafe extern "C" fn shmem_destroy_callback<T: Operations>(
    _cookie: *mut core::ffi::c_void,
    bfr: *mut bindings::apple_rtkit_shmem,
) {
    let bfr_mut = unsafe { &mut *bfr };
    // SAFETY: Per shmem_setup_callback, this has to be a pointer to a Buffer if it is set.
    if !bfr_mut.private.is_null() {
        unsafe {
            core::mem::drop(Box::from_raw(bfr_mut.private as *mut T::Buffer));
        }
        bfr_mut.private = core::ptr::null_mut();
    }
}

impl<T: Operations> RtKit<T> {
    const VTABLE: bindings::apple_rtkit_ops = bindings::apple_rtkit_ops {
        crashed: Some(crashed_callback::<T>),
        recv_message: Some(recv_message_callback::<T>),
        recv_message_early: Some(recv_message_early_callback::<T>),
        shmem_setup: if T::HAS_SHMEM_ALLOC || T::HAS_SHMEM_MAP {
            Some(shmem_setup_callback::<T>)
        } else {
            None
        },
        shmem_destroy: if T::HAS_SHMEM_ALLOC || T::HAS_SHMEM_MAP {
            Some(shmem_destroy_callback::<T>)
        } else {
            None
        },
    };

    /// Creates a new RTKit client for a given device and optional mailbox name or index.
    pub fn new(
        dev: &dyn device::RawDevice,
        mbox_name: Option<&'static CStr>,
        mbox_idx: usize,
        data: T::Data,
    ) -> Result<Self> {
        let ptr = data.into_foreign() as *mut _;
        let guard = ScopeGuard::new(|| {
            // SAFETY: `ptr` came from a previous call to `into_foreign`.
            unsafe { T::Data::from_foreign(ptr) };
        });
        // SAFETY: This just calls the C init function.
        let rtk = unsafe {
            from_kernel_err_ptr(bindings::apple_rtkit_init(
                dev.raw_device(),
                ptr,
                match mbox_name {
                    Some(s) => s.as_char_ptr(),
                    None => ptr::null(),
                },
                mbox_idx.try_into()?,
                &Self::VTABLE,
            ))
        }?;

        guard.dismiss();
        // INVARIANT: `rtk` and `data` are valid here.
        Ok(Self {
            rtk,
            data: ptr,
            _p: PhantomData,
        })
    }

    /// Boots (wakes up) the RTKit coprocessor.
    pub fn boot(&mut self) -> Result {
        // SAFETY: `rtk` is valid per the type invariant.
        to_result(unsafe { bindings::apple_rtkit_boot(self.rtk) })
    }

    /// Starts a non-system endpoint.
    pub fn start_endpoint(&mut self, endpoint: u8) -> Result {
        // SAFETY: `rtk` is valid per the type invariant.
        to_result(unsafe { bindings::apple_rtkit_start_ep(self.rtk, endpoint) })
    }

    /// Sends a message to a given endpoint.
    pub fn send_message(&mut self, endpoint: u8, message: u64) -> Result {
        // SAFETY: `rtk` is valid per the type invariant.
        to_result(unsafe {
            bindings::apple_rtkit_send_message(self.rtk, endpoint, message, ptr::null_mut(), false)
        })
    }
}

// SAFETY: `RtKit` operations require a mutable reference
unsafe impl<T: Operations> Sync for RtKit<T> {}

// SAFETY: `RtKit` operations require a mutable reference
unsafe impl<T: Operations> Send for RtKit<T> {}

impl<T: Operations> Drop for RtKit<T> {
    fn drop(&mut self) {
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::apple_rtkit_free(self.rtk) };

        // Free context data.
        //
        // SAFETY: This matches the call to `into_foreign` from `new` in the success case.
        unsafe { T::Data::from_foreign(self.data) };
    }
}
