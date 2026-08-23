// SPDX-License-Identifier: GPL-2.0

//! Watchdog Timer Subsystem abstractions for Rust.
//!
//! C header: [`include/linux/watchdog.h`](srctree/include/linux/watchdog.h)

use crate::{
    alloc::KBox,
    bindings,
    device,
    error::*,
    ffi::{c_int, c_uint, c_void},
    prelude::*,
};
use core::{marker::PhantomData, ptr::NonNull};

/// Watchdog option flags (from `uapi/linux/watchdog.h`).
pub struct WatchdogFlags;

impl WatchdogFlags {
    /// Keepalive ping support.
    pub const KEEPALIVEPING: u32 = bindings::WDIOF_KEEPALIVEPING;
    /// Set timeout support.
    pub const SETTIMEOUT: u32 = bindings::WDIOF_SETTIMEOUT;
    /// Magic close support.
    pub const MAGICCLOSE: u32 = bindings::WDIOF_MAGICCLOSE;
}

/// Operations trait for watchdog device drivers.
pub trait Operations: Send + Sync + Sized {
    /// Start the watchdog timer.
    fn start(&self) -> Result;

    /// Stop the watchdog timer.
    fn stop(&self) -> Result {
        Err(EINVAL)
    }

    /// Send keepalive ping to reset watchdog counter.
    fn ping(&self) -> Result {
        self.start()
    }

    /// Set timeout value in seconds.
    fn set_timeout(&self, timeout: u32) -> Result {
        let _ = timeout;
        Err(EINVAL)
    }
}

/// Adapter holding C trampolines for watchdog operations.
pub struct Adapter<T: Operations>(PhantomData<T>);

impl<T: Operations> Adapter<T> {
    unsafe extern "C" fn start_callback(wdd: *mut bindings::watchdog_device) -> c_int {
        if wdd.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { (*wdd).driver_data };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.start() {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn stop_callback(wdd: *mut bindings::watchdog_device) -> c_int {
        if wdd.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { (*wdd).driver_data };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.stop() {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn ping_callback(wdd: *mut bindings::watchdog_device) -> c_int {
        if wdd.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { (*wdd).driver_data };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.ping() {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn set_timeout_callback(
        wdd: *mut bindings::watchdog_device,
        timeout: c_uint,
    ) -> c_int {
        if wdd.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { (*wdd).driver_data };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.set_timeout(timeout as u32) {
            Ok(()) => {
                unsafe { (*wdd).timeout = timeout };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    /// Const vtable for watchdog ops.
    pub const OPS: bindings::watchdog_ops = bindings::watchdog_ops {
        owner: core::ptr::null_mut(),
        start: Some(Self::start_callback),
        stop: Some(Self::stop_callback),
        ping: Some(Self::ping_callback),
        status: None,
        set_timeout: Some(Self::set_timeout_callback),
        set_pretimeout: None,
        get_timeleft: None,
        restart: None,
        ioctl: None,
    };
}

/// Registration handle for a watchdog device.
pub struct Registration {
    wdd: NonNull<bindings::watchdog_device>,
}

// SAFETY: Watchdog registration can be transferred across threads.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

impl Registration {
    /// Register a new watchdog timer with the kernel.
    pub fn register<T: Operations>(
        parent_dev: &device::Device,
        info: &'static bindings::watchdog_info,
        drvdata: &T,
        min_timeout: u32,
        max_timeout: u32,
        default_timeout: u32,
    ) -> Result<Self> {
        let mut wdd_box: KBox<bindings::watchdog_device> = KBox::zeroed(GFP_KERNEL)?;

        wdd_box.parent = parent_dev.as_raw();
        wdd_box.info = info;
        wdd_box.ops = &Adapter::<T>::OPS;
        wdd_box.min_timeout = min_timeout;
        wdd_box.max_timeout = max_timeout;
        wdd_box.timeout = default_timeout;
        wdd_box.driver_data = (drvdata as *const T).cast::<c_void>() as *mut c_void;

        let wdd_ptr = KBox::into_raw(wdd_box);
        let parent_ptr = parent_dev.as_raw();

        // SAFETY: We pass valid pointers to devm_watchdog_register_device.
        let ret = unsafe { bindings::devm_watchdog_register_device(parent_ptr, wdd_ptr) };
        if ret < 0 {
            unsafe { drop(KBox::from_raw(wdd_ptr)) };
            return Err(Error::from_errno(ret));
        }

        let non_null = NonNull::new(wdd_ptr).ok_or(ENOMEM)?;
        Ok(Self { wdd: non_null })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            drop(KBox::from_raw(self.wdd.as_ptr()));
        }
    }
}
