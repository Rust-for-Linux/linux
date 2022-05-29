/// Device run-time power management helper functions.
///
/// C header: [`include/linux/pm_runtime.h`](../../../../../include/linux/pm_runtime.h)
use crate::{
    bindings,
    device::{self, RawDevice},
    to_result, Result,
};

/// Enable runtime PM of device `dev`.
pub fn enable(dev: &impl RawDevice) -> DisableGuard {
    // SAFETY: Satisfied by the safety requirements of `RawDevice`.
    unsafe { bindings::pm_runtime_enable(dev.raw_device()) };
    DisableGuard::new(device::Device::from_dev(dev))
}

/// Guard that disables runtime PM of device `dev`.
pub struct DisableGuard {
    dev: device::Device,
}

impl DisableGuard {
    /// Create new instance of a guard of device `dev`.
    fn new(dev: device::Device) -> Self {
        Self { dev }
    }
}

impl Drop for DisableGuard {
    fn drop(&mut self) {
        // SAFETY: Satisfied by the safety requirements of `RawDevice`.
        unsafe { bindings::pm_runtime_disable(self.dev.raw_device()) };
    }
}

/// Drop runtime PM usage counter of a device.
/// Decrement the runtime PM usage counter of `dev` unless it is 0 already.
pub fn put_noidle(dev: &impl RawDevice) {
    // SAFETY: Satisfied by the safety requirements of `RawDevice`.
    unsafe { bindings::pm_runtime_put_noidle(dev.raw_device()) };
}

/// Drop device `dev` usage counter and run "idle check" if 0.
///
/// Decrement the runtime PM usage counter of `dev` and if it turns out to be
/// equal to 0, invoke the "idle check" callback of `dev` and, depending on its
/// return value, set up autosuspend of `dev` or suspend it (depending on whether
/// or not autosuspend has been enabled for it).
///
/// The possible return values of this function are the same as for
/// pm_runtime_idle() and the runtime PM usage counter of `dev` remains
/// decremented in all cases, even when an error is returned.
pub fn put_sync(dev: &impl RawDevice) {
    // SAFETY: Satisfied by the safety requirements of `RawDevice`.
    unsafe { bindings::pm_runtime_put_sync(dev.raw_device()) };
}

/// Bump up usage counter of a device `dev` and resume it.
///
/// Resume `dev` synchronously and if that is successful, increment its runtime
/// PM usage counter. Return error if the runtime PM usage counter of `dev`
/// has not been incremented.
pub fn resume_and_get(dev: &impl RawDevice) -> Result {
    // SAFETY: Satisfied by the safety requirements of `RawDevice`.
    to_result(|| unsafe { bindings::pm_runtime_resume_and_get(dev.raw_device()) })
}
