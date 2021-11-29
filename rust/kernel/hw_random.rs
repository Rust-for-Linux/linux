// SPDX-License-Identifier: GPL-2.0

//! Hardware Random Number Generator
//!
//! C header: [`include/linux/hw_random.h`](../../../../include/linux/hw_random.h)

use alloc::{boxed::Box, slice::from_raw_parts_mut};

use crate::{
    bindings, c_types, error::from_kernel_result, str::CStr, types::PointerWrapper, Error, Result,
};

use core::{cell::UnsafeCell, marker::PhantomData, pin::Pin};

/// This trait is implemented in order to provide callbacks to `struct hwrng`.
pub trait HwrngOperations: Sized + 'static {
    /// The methods to use to populate [`struct hwrng`].
    const TO_USE: ToUse;

    /// The pointer type that will be used to hold user-defined data type.
    type Data: PointerWrapper = Box<Self>;

    /// Initialization callback, can be left undefined.
    fn init(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Result {
        Err(Error::EINVAL)
    }

    /// Cleanup callback, can be left undefined.
    fn cleanup(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) {}

    /// Read data into the provided buffer.
    /// Drivers can fill up to max bytes of data into the buffer.
    /// The buffer is aligned for any type and max is a multiple of 4 and >= 32 bytes.
    fn read(
        data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        buffer: &mut [i8],
        wait: bool,
    ) -> Result<i32>;
}

/// Registration structure for Hardware Random Number Generator driver.
pub struct Registration<T: HwrngOperations> {
    hwrng: UnsafeCell<bindings::hwrng>,
    _p: PhantomData<T>,
}

impl<'a, T: HwrngOperations> Registration<T> {
    fn init_hwrng(
        hwrng: &mut bindings::hwrng,
        name: &'a CStr,
        quality: u16,
        data: *const core::ffi::c_void,
    ) {
        hwrng.name = name.as_char_ptr();

        hwrng.init = if T::TO_USE.init {
            Some(init_callback::<T>)
        } else {
            None
        };
        hwrng.cleanup = if T::TO_USE.cleanup {
            Some(cleanup_callback::<T>)
        } else {
            None
        };
        hwrng.data_present = None;
        hwrng.data_read = None;
        hwrng.read = Some(read_callback::<T>);

        hwrng.priv_ = data as _;
        hwrng.quality = quality;

        // SAFETY: All fields are properly initialized as
        // remaining fields `list`, `ref` and `cleanup_done` are already
        // zeroed by `bindings::hwrng::default()` call.
    }

    /// Registers a hwrng device within the rest of the kernel.
    ///
    /// It must be pinned because the memory block that represents the registration.
    fn register(&self) -> Result {
        // SAFETY: register function requires initialized `bindings::hwrng`
        // It is called in `Registration<T>::new_pinned` after initialization of the structure
        // that quarantees safety.
        let ret = unsafe { bindings::hwrng_register(&mut *self.hwrng.get()) };

        if ret < 0 {
            return Err(Error::from_kernel_errno(ret));
        }
        Ok(())
    }

    /// Returns a registered and pinned, heap-allocated representation of the registration.
    pub fn new_pinned(name: &'a CStr, quality: u16, data: T::Data) -> Result<Pin<Box<Self>>> {
        let data_pointer = <T::Data as PointerWrapper>::into_pointer(data);

        let registeration = Self {
            hwrng: UnsafeCell::new(bindings::hwrng::default()),
            _p: PhantomData::default(),
        };

        // SAFETY: registeration contains allocated and set to zero `bindings::hwrng` structure.
        Self::init_hwrng(
            unsafe { &mut *registeration.hwrng.get() },
            name,
            quality,
            data_pointer,
        );

        let pinned = Pin::from(Box::try_new(registeration)?);

        pinned.as_ref().register()?;

        Ok(pinned)
    }
}

/// Represents which callbacks of [`struct hwrng`] should be populated with pointers.
pub struct ToUse {
    /// The `init` field of [`struct hwrng`].
    pub init: bool,

    /// The `cleanup` field of [`struct hwrng`].
    pub cleanup: bool,
}

/// A constant version where all values are to set to `false`, that is, all supported fields will
/// be set to null pointers.
pub const USE_NONE: ToUse = ToUse {
    init: false,
    cleanup: false,
};

/// Defines the [`HwrngOperations::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_hwrng_operations {
    () => {
        const TO_USE: $crate::hw_random::ToUse = $crate::hw_random::USE_NONE;
    };
    ($($i:ident),+) => {
        const TO_USE: kernel::hw_random::ToUse =
            $crate::hw_random::ToUse {
                $($i: true),+ ,
                ..$crate::hw_random::USE_NONE
            };
    };
}

unsafe extern "C" fn init_callback<T: HwrngOperations>(
    rng: *mut bindings::hwrng,
) -> c_types::c_int {
    from_kernel_result! {
        // SAFETY: `priv` private data field was initialized during creation of
        // the `bindings::hwrng` in `Self::init_hwrng` function. This callback
        // is only called once `new_pinned` suceeded previously which guarantees safety.
        let data = unsafe { T::Data::borrow((*rng).priv_ as *const core::ffi::c_void) };
        T::init(data)?;
        Ok(0)
    }
}

unsafe extern "C" fn cleanup_callback<T: HwrngOperations>(rng: *mut bindings::hwrng) {
    // SAFETY: `priv` private data field was initialized during creation of
    // the `bindings::hwrng` in `Self::init_hwrng` function. This callback
    // is only called once `new_pinned` suceeded previously which guarantees safety.
    let data = unsafe { T::Data::borrow((*rng).priv_ as *const core::ffi::c_void) };
    T::cleanup(data);
}

unsafe extern "C" fn read_callback<T: HwrngOperations>(
    rng: *mut bindings::hwrng,
    data: *mut c_types::c_void,
    max: usize,
    wait: bindings::bool_,
) -> c_types::c_int {
    from_kernel_result! {
        // SAFETY: `priv` private data field was initialized during creation of
        // the `bindings::hwrng` in `Self::init_hwrng` function. This callback
        // is only called once `new_pinned` suceeded previously which guarantees safety.
        let drv_data = unsafe { T::Data::borrow((*rng).priv_ as *const core::ffi::c_void) };

        // SAFETY: Slice is created from `data` and `max` arguments that are C api buffer
        // and its size in bytes which are safe for this conversion.
        let buffer = unsafe { from_raw_parts_mut(data as *mut c_types::c_char, max) };
        let ret = T::read(drv_data, buffer, wait)?;
        Ok(ret)
    }
}

// SAFETY: `Registration` does not expose any of its state across threads
unsafe impl<T: HwrngOperations> Sync for Registration<T> {}

impl<T: HwrngOperations> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        // SAFETY: The instance of Registration<T> is returned from `new_pinned`
        // function after being initialized and registered.
        unsafe { bindings::hwrng_unregister(&mut *self.hwrng.get()) };
    }
}
