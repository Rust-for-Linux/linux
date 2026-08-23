// SPDX-License-Identifier: GPL-2.0

//! Non-Volatile Memory (NVMEM) Subsystem abstractions for Rust.
//!
//! C header: [`include/linux/nvmem-provider.h`](srctree/include/linux/nvmem-provider.h)

use crate::{
    alloc::KBox,
    bindings,
    device,
    error::*,
    ffi::{c_int, c_uint, c_void},
    prelude::*,
};
use core::{marker::PhantomData, ptr::NonNull};

/// NVMEM storage type enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum NvmemType {
    /// Unknown or generic storage.
    Unknown = bindings::nvmem_type_NVMEM_TYPE_UNKNOWN,
    /// EEPROM storage.
    Eeprom = bindings::nvmem_type_NVMEM_TYPE_EEPROM,
    /// One-Time Programmable storage.
    Otp = bindings::nvmem_type_NVMEM_TYPE_OTP,
    /// Battery-backed RAM.
    BatteryBacked = bindings::nvmem_type_NVMEM_TYPE_BATTERY_BACKED,
    /// Ferroelectric RAM.
    Fram = bindings::nvmem_type_NVMEM_TYPE_FRAM,
}

/// Operations trait for NVMEM providers.
pub trait Operations: Send + Sync + Sized {
    /// Read `buf.len()` bytes starting at `offset`.
    fn read(&self, offset: u32, buf: &mut [u8]) -> Result;

    /// Write `buf.len()` bytes starting at `offset`.
    fn write(&self, offset: u32, buf: &[u8]) -> Result {
        let _ = (offset, buf);
        Err(EINVAL)
    }
}

/// Adapter holding C trampolines for NVMEM operations.
pub struct Adapter<T: Operations>(PhantomData<T>);

impl<T: Operations> Adapter<T> {
    unsafe extern "C" fn reg_read_callback(
        priv_: *mut c_void,
        offset: c_uint,
        val: *mut c_void,
        bytes: usize,
    ) -> c_int {
        if priv_.is_null() || val.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*priv_.cast::<T>() };
        // SAFETY: `val` points to buffer of at least `bytes` length.
        let buf = unsafe { core::slice::from_raw_parts_mut(val.cast::<u8>(), bytes) };
        match op.read(offset as u32, buf) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn reg_write_callback(
        priv_: *mut c_void,
        offset: c_uint,
        val: *mut c_void,
        bytes: usize,
    ) -> c_int {
        if priv_.is_null() || val.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*priv_.cast::<T>() };
        // SAFETY: `val` points to buffer of at least `bytes` length.
        let buf = unsafe { core::slice::from_raw_parts(val.cast::<u8>(), bytes) };
        match op.write(offset as u32, buf) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }
}

/// Registration handle for a registered NVMEM device.
pub struct Registration {
    #[expect(dead_code)]
    nvmem: NonNull<bindings::nvmem_device>,
}

// SAFETY: NVMEM registration can be transferred across threads.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

impl Registration {
    /// Register a new NVMEM device with the kernel.
    pub fn register<T: Operations>(
        parent_dev: &device::Device,
        name: &'static CStr,
        drvdata: &T,
        size: usize,
        nvmem_type: NvmemType,
        read_only: bool,
    ) -> Result<Self> {
        let mut config: KBox<bindings::nvmem_config> = KBox::zeroed(GFP_KERNEL)?;

        config.dev = parent_dev.as_raw();
        config.name = name.as_char_ptr();
        config.id = bindings::NVMEM_DEVID_AUTO as i32;
        config.size = size as c_int;
        config.word_size = 1;
        config.stride = 1;
        config.type_ = nvmem_type as u32;
        config.read_only = read_only;
        config.priv_ = (drvdata as *const T).cast::<c_void>() as *mut c_void;
        config.reg_read = Some(Adapter::<T>::reg_read_callback);
        config.reg_write = Some(Adapter::<T>::reg_write_callback);

        let parent_ptr = parent_dev.as_raw();

        // SAFETY: `devm_nvmem_register` copies the fields it needs from `config`.
        let ret = from_err_ptr(unsafe { bindings::devm_nvmem_register(parent_ptr, &*config) })?;
        let non_null = NonNull::new(ret).ok_or(ENOMEM)?;
        Ok(Self { nvmem: non_null })
    }
}
