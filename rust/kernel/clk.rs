// SPDX-License-Identifier: GPL-2.0

//! Common clock framework.
//!
//! C header: [`include/linux/clk.h`](../../../../include/linux/clk.h)

use crate::{bindings, error::Result, to_result};
use core::mem::ManuallyDrop;

/// Represents `struct clk *`.
///
/// # Invariants
///
/// The pointer is valid.
pub struct Clk(*mut bindings::clk);

impl Clk {
    /// Creates new clock structure from a raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be valid.
    pub unsafe fn new(clk: *mut bindings::clk) -> Self {
        Self(clk)
    }

    /// Returns value of the rate field of `struct clk`.
    pub fn get_rate(&self) -> usize {
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::clk_get_rate(self.0) as usize }
    }

    /// Prepares and enables the underlying hardware clock.
    ///
    /// This function should not be called in atomic context.
    pub fn prepare_enable(self) -> Result<EnabledClk> {
        // SAFETY: The pointer is valid by the type invariant.
        to_result(unsafe { bindings::clk_prepare_enable(self.0) })?;
        Ok(EnabledClk(self))
    }
}

impl Drop for Clk {
    fn drop(&mut self) {
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::clk_put(self.0) };
    }
}

// SAFETY: `Clk` is not restricted to a single thread so it is safe
// to move it between threads.
unsafe impl Send for Clk {}

/// A clock variant that is prepared and enabled.
pub struct EnabledClk(Clk);

impl EnabledClk {
    /// Returns value of the rate field of `struct clk`.
    pub fn get_rate(&self) -> usize {
        self.0.get_rate()
    }

    /// Disables and later unprepares the underlying hardware clock prematurely.
    ///
    /// This function should not be called in atomic context.
    pub fn disable_unprepare(self) -> Clk {
        let mut clk = ManuallyDrop::new(self);
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::clk_disable_unprepare(clk.0 .0) };
        core::mem::replace(&mut clk.0, Clk(core::ptr::null_mut()))
    }
}

impl Drop for EnabledClk {
    fn drop(&mut self) {
        // SAFETY: The pointer is valid by the type invariant.
        unsafe { bindings::clk_disable_unprepare(self.0 .0) };
    }
}
