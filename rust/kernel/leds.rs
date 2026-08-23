// SPDX-License-Identifier: GPL-2.0

//! LED Class Subsystem abstractions for Rust.
//!
//! C header: [`include/linux/leds.h`](srctree/include/linux/leds.h)

use crate::{
    alloc::KBox,
    bindings,
    device,
    error::*,
    ffi::c_int,
    prelude::*,
};
use core::{marker::PhantomData, ptr::NonNull};

/// LED brightness levels.
pub struct Brightness;

impl Brightness {
    /// LED turned completely off (0).
    pub const OFF: u32 = bindings::led_brightness_LED_OFF;
    /// LED at half brightness (127).
    pub const HALF: u32 = bindings::led_brightness_LED_HALF;
    /// LED at full maximum brightness (255).
    pub const FULL: u32 = bindings::led_brightness_LED_FULL;
}

/// Operations trait implemented by LED drivers.
pub trait Operations: Send + Sync + Sized {
    /// Set the brightness of the LED.
    fn brightness_set(&self, brightness: u32) -> Result;

    /// Get current brightness of the LED (optional).
    fn brightness_get(&self) -> Result<u32> {
        Err(EINVAL)
    }

    /// Activate hardware blinking with specific on/off delays in milliseconds (optional).
    fn blink_set(&self, delay_on: &mut usize, delay_off: &mut usize) -> Result {
        let _ = (delay_on, delay_off);
        Err(EINVAL)
    }
}

/// Adapter holding C trampolines for LED class callbacks.
pub struct Adapter<T: Operations>(PhantomData<T>);

impl<T: Operations> Adapter<T> {
    unsafe extern "C" fn brightness_set_blocking_callback(
        led_cdev: *mut bindings::led_classdev,
        brightness: bindings::led_brightness,
    ) -> c_int {
        if led_cdev.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        // SAFETY: `led_cdev.dev` has drvdata pointing to `T`.
        let dev = unsafe { (*led_cdev).dev };
        if dev.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.brightness_set(brightness as u32) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn brightness_get_callback(
        led_cdev: *mut bindings::led_classdev,
    ) -> bindings::led_brightness {
        if led_cdev.is_null() {
            return 0;
        }
        let dev = unsafe { (*led_cdev).dev };
        if dev.is_null() {
            return 0;
        }
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() {
            return 0;
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        match op.brightness_get() {
            Ok(b) => b as bindings::led_brightness,
            Err(_) => 0,
        }
    }

    unsafe extern "C" fn blink_set_callback(
        led_cdev: *mut bindings::led_classdev,
        delay_on: *mut usize,
        delay_off: *mut usize,
    ) -> c_int {
        if led_cdev.is_null() || delay_on.is_null() || delay_off.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let dev = unsafe { (*led_cdev).dev };
        if dev.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let drvdata = unsafe { bindings::dev_get_drvdata(dev) };
        if drvdata.is_null() {
            return -(bindings::EINVAL as c_int);
        }
        let op = unsafe { &*drvdata.cast::<T>() };
        let mut on = unsafe { *delay_on };
        let mut off = unsafe { *delay_off };

        match op.blink_set(&mut on, &mut off) {
            Ok(()) => {
                unsafe {
                    *delay_on = on;
                    *delay_off = off;
                }
                0
            }
            Err(e) => e.to_errno(),
        }
    }
}

/// Registration handle for a registered LED class device.
pub struct Registration {
    cdev: NonNull<bindings::led_classdev>,
}

// SAFETY: Registration can be sent across threads.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

impl Registration {
    /// Register a new LED device with the kernel LED subsystem.
    pub fn register<T: Operations>(
        parent_dev: &device::Device,
        name: &'static CStr,
        _drvdata: &T,
        max_brightness: u32,
    ) -> Result<Self> {
        let mut cdev_box: KBox<bindings::led_classdev> = KBox::zeroed(GFP_KERNEL)?;

        cdev_box.name = name.as_char_ptr();
        cdev_box.max_brightness = max_brightness;
        cdev_box.brightness_set_blocking = Some(Adapter::<T>::brightness_set_blocking_callback);
        cdev_box.brightness_get = Some(Adapter::<T>::brightness_get_callback);
        cdev_box.blink_set = Some(Adapter::<T>::blink_set_callback);

        let cdev_ptr = KBox::into_raw(cdev_box);
        let parent_ptr = parent_dev.as_raw();

        // SAFETY: `cdev_ptr` is valid heap memory and `parent_ptr` is a valid device pointer.
        let ret = unsafe {
            bindings::devm_led_classdev_register_ext(
                parent_ptr,
                cdev_ptr,
                core::ptr::null_mut(),
            )
        };

        if ret < 0 {
            // SAFETY: Re-acquire ownership to deallocate on failure.
            unsafe { drop(KBox::from_raw(cdev_ptr)) };
            return Err(Error::from_errno(ret));
        }

        let non_null = NonNull::new(cdev_ptr).ok_or(ENOMEM)?;
        Ok(Self { cdev: non_null })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: We deallocate the KBox allocated during registration.
        unsafe {
            drop(KBox::from_raw(self.cdev.as_ptr()));
        }
    }
}
