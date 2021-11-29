// SPDX-License-Identifier: GPL-2.0

//! Clock subsystem
//!
//! C header: [`include/linux/clk.h`](../../../../include/linux/clk.h)

use crate::{
    bindings, device,
    error::{from_kernel_err_ptr, Error, Result},
    str::CStr,
};

/// Represents `struct clk *`
///
/// # Invariants
///
/// The pointer is valid.
pub struct Clk(*mut bindings::clk);

impl Clk {
    /// Returns value of rate field of `struct clk`.
    pub fn get_rate(&self) -> u32 {
        // SAFETY: the pointer is valid by the type invariant.
        unsafe { bindings::clk_get_rate((*self).0) }
    }

    /// Do not use in atomic context.
    pub fn prepare_enable(&mut self) -> Result {
        // SAFETY: the pointer is valid by the type invariant.
        let ret = unsafe { bindings::clk_prepare_enable((*self).0) };
        if ret < 0 {
            return Err(Error::from_kernel_errno(ret));
        }

        Ok(())
    }
}

/// Lookup and obtain a managed reference to a clock producer.
///
/// * `dev` - device for clock "consumer".
/// * `id` - clock consumer ID.
pub fn devm_clk_get(dev: &impl device::RawDevice, id: &'static CStr) -> Result<Clk> {
    // SAFETY:
    //   - dev is valid by the safety contract.
    //   - id is valid by the type invariant.
    let clk_ptr =
        unsafe { from_kernel_err_ptr(bindings::devm_clk_get(dev.raw_device(), id.as_char_ptr())) }?;

    Ok(Clk(clk_ptr))
}
