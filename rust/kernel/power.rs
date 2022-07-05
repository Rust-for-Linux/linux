// SPDX-License-Identifier: GPL-2.0

//! Power management interfaces.
//!
//! C header: [`include/linux/pm.h`](../../../../include/linux/pm.h)

#![allow(dead_code)]

use crate::{bindings, error::from_kernel_result, types::PointerWrapper, Result};
use core::marker::PhantomData;

/// Corresponds to the kernel's `struct dev_pm_ops`.
///
/// It is meant to be implemented by drivers that support power-management operations.
pub trait Operations {
    /// The type of the context data stored by the driver on each device.
    type Data: PointerWrapper + Sync + Send;

    /// Called before the system goes into a sleep state.
    fn suspend(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Result {
        Ok(())
    }

    /// Called after the system comes back from a sleep state.
    fn resume(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Result {
        Ok(())
    }

    /// Called before creating a hibernation image.
    fn freeze(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Result {
        Ok(())
    }

    /// Called after the system is restored from a hibernation image.
    fn restore(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Result {
        Ok(())
    }
}

macro_rules! pm_callback {
    ($callback:ident, $method:ident) => {
        unsafe extern "C" fn $callback<T: Operations>(
            dev: *mut bindings::device,
        ) -> core::ffi::c_int {
            from_kernel_result! {
                // SAFETY: `dev` is valid as it was passed in by the C portion.
                let ptr = unsafe { bindings::dev_get_drvdata(dev) };
                // SAFETY: By the safety requirements of `OpsTable::build`, we know that `ptr` came
                // from a previous call to `T::Data::into_pointer`.
                let data = unsafe { T::Data::borrow(ptr) };
                T::$method(data)?;
                Ok(0)
            }
        }
    };
}

pm_callback!(suspend_callback, suspend);
pm_callback!(resume_callback, resume);
pm_callback!(freeze_callback, freeze);
pm_callback!(restore_callback, restore);

pub(crate) struct OpsTable<T: Operations>(PhantomData<*const T>);

impl<T: Operations> OpsTable<T> {
    const VTABLE: bindings::dev_pm_ops = bindings::dev_pm_ops {
        prepare: None,
        complete: None,
        suspend: Some(suspend_callback::<T>),
        resume: Some(resume_callback::<T>),
        freeze: Some(freeze_callback::<T>),
        thaw: None,
        poweroff: None,
        restore: Some(restore_callback::<T>),
        suspend_late: None,
        resume_early: None,
        freeze_late: None,
        thaw_early: None,
        poweroff_late: None,
        restore_early: None,
        suspend_noirq: None,
        resume_noirq: None,
        freeze_noirq: None,
        thaw_noirq: None,
        poweroff_noirq: None,
        restore_noirq: None,
        runtime_suspend: None,
        runtime_resume: None,
        runtime_idle: None,
    };

    /// Builds an instance of `struct dev_pm_ops`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `dev_get_drvdata` will result in a value returned by
    /// [`T::Data::into_pointer`].
    pub(crate) const unsafe fn build() -> &'static bindings::dev_pm_ops {
        &Self::VTABLE
    }
}

/// Implements the [`Operations`] trait as no-ops.
///
/// This is useful when one doesn't want to provide the implementation of any power-manager related
/// operation.
pub struct NoOperations<T: PointerWrapper>(PhantomData<T>);

impl<T: PointerWrapper + Send + Sync> Operations for NoOperations<T> {
    type Data = T;
}

// SAFETY: `NoOperation` provides no functionality, it is safe to send a reference to it to
// different threads.
unsafe impl<T: PointerWrapper> Sync for NoOperations<T> {}

// SAFETY: `NoOperation` provides no functionality, it is safe to send it to different threads.
unsafe impl<T: PointerWrapper> Send for NoOperations<T> {}
