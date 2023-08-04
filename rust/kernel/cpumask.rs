// SPDX-License-Identifier: GPL-2.0

//! Cpumask variables and related functions.
//!
//! C header: [`include/linux/cpumask.h`](../../../../include/linux/cpumask.h).

use crate::bindings;
use core::iter::Iterator;

/// A valid CPU index.
///
/// # Safety
///
/// - The 'ValidCpuIndex' should be used during the iteration of a CPU index iterator.
pub struct ValidCpuIndex(u32);

impl ValidCpuIndex {
    /// Get the valid CPU index in u32.
    pub fn get(&self) -> u32 {
        self.0
    }
}

/// An possible CPU index iterator.
///
/// This iterator has a similar abilitiy to the kernel's macro `for_each_possible_cpu`.
pub struct PossibleCpusIndexIter {
    index: i32,
}

/// An online CPU index iterator.
///
/// This iterator has a similar abilitiy to the kernel's macro `for_each_online_cpu`.
pub struct OnlineCpusIndexIter {
    index: i32,
}

/// An present CPU index iterator.
///
/// This iterator has a similar abilitiy to the kernel's macro `for_each_present_cpu`.
pub struct PresentCpusIndexIter {
    index: i32,
}

impl Iterator for PossibleCpusIndexIter {
    type Item = ValidCpuIndex;

    fn next(&mut self) -> Option<ValidCpuIndex> {
        let next_cpu_id =
            // SAFETY: Since [`bindings::__cpu_possible_mask`] will not change, there will not
            // be data race in this part. When the last valid CPU index is found, this iterator
            // will return `None`. Therefore, the index parameter is always valid.
            unsafe { bindings::cpumask_next(self.index, &bindings::__cpu_possible_mask) };
        // When [`bindings::cpumask_next`] can not find further CPUs set in the
        // [`bindings::__cpu_possible_mask`], it returns a value >= [`bindings::nr_cpu_ids`].
        //
        // SAFETY: The [`bindings::nr_cpu_ids`] is fixed at the boot time.
        if next_cpu_id >= unsafe { bindings::nr_cpu_ids } {
            return None;
        }
        self.index = next_cpu_id as i32;
        Some(ValidCpuIndex(next_cpu_id))
    }
}

impl Iterator for OnlineCpusIndexIter {
    type Item = ValidCpuIndex;

    fn next(&mut self) -> Option<ValidCpuIndex> {
        #[cfg(CONFIG_HOTPLUG_CPU)]
        if self.index == -1 {
            // The [`bindings::__cpu_online_mask`] and [`bindings::nr_cpu_ids`] may chanage if
            // `CONFIG_HOTPLUG_CPU` is enabled. In case of race condition, a lock is needed
            // here. If `CONFIG_HOTPLUG_CPU` is disabled, this function will not have any cost.
            //
            // SAFETY: FFI call, this is called once during iteration in case of dead lock.
            unsafe { bindings::cpus_read_lock() };
        }
        let next_cpu_id =
            // SAFETY: The [`bindings::cpus_read_lock`] prevents the data race. When the last 
            // valid CPU index is found, this iterator will return `None`. Therefore, the 
            // index parameter is always valid.
            unsafe { bindings::cpumask_next(self.index, &bindings::__cpu_online_mask) };
        // When [`bindings::cpumask_next`] can not find further CPUs set in the
        // [`bindings::__cpu_online_mask`], it returns a value >= [`bindings::nr_cpu_ids`].
        //
        // SAFETY: The [`bindings::nr_cpu_ids`] is fixed at the boot time.
        if next_cpu_id >= unsafe { bindings::nr_cpu_ids } {
            // Unlock after finishing iteration.
            //
            // SAFETY: FFI call.
            #[cfg(CONFIG_HOTPLUG_CPU)]
            unsafe {
                bindings::cpus_read_unlock()
            };
            return None;
        }
        self.index = next_cpu_id as i32;
        Some(ValidCpuIndex(next_cpu_id))
    }
}

impl Iterator for PresentCpusIndexIter {
    type Item = ValidCpuIndex;

    fn next(&mut self) -> Option<ValidCpuIndex> {
        #[cfg(CONFIG_HOTPLUG_CPU)]
        if self.index == -1 {
            // The [`bindings::__cpu_present_mask`] and [`bindings::nr_cpu_ids`] may chanage
            // if `CONFIG_HOTPLUG_CPU` is enabled. In case of race condition, a lock is needed
            // here. If `CONFIG_HOTPLUG_CPU` is disabled, this function will not have any cost.
            //
            // SAFETY: FFI call, this is called once during iteration in case of dead lock.
            unsafe { bindings::cpus_read_lock() };
        }
        let next_cpu_id =
            // SAFETY: The [`bindings::cpus_read_lock`] prevents the data race. When the last 
            // valid CPU index is found, this iterator will return `None`. Therefore, the 
            // index parameter is always valid.
            unsafe { bindings::cpumask_next(self.index, &bindings::__cpu_present_mask) };
        // When [`bindings::cpumask_next`] can not find further CPUs set in the
        // [`bindings::__cpu_present_mask`], it returns a value >= [`bindings::nr_cpu_ids`].
        //
        // SAFETY: The [`bindings::nr_cpu_ids`] is fixed at the boot time.
        if next_cpu_id >= unsafe { bindings::nr_cpu_ids } {
            // Unlock after finishing iteration.
            //
            // SAFETY: FFI call.
            #[cfg(CONFIG_HOTPLUG_CPU)]
            unsafe {
                bindings::cpus_read_unlock()
            };
            return None;
        }
        self.index = next_cpu_id as i32;
        Some(ValidCpuIndex(next_cpu_id))
    }
}

/// Returns a [`PossibleCpusIndexIter`] that gives the possible CPU indexes.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::cpumask::possible_cpus;
///
/// fn example() {
///     // This prints all the possible cpu indexes.
///     for cpu in possible_cpus(){
///         pr_info!("{}\n", cpu.get());
///     }
/// }
/// ```
pub fn possible_cpus() -> PossibleCpusIndexIter {
    // Initial index is set to -1. Since [`bindings::cpumask_next`] return the next set bit in a
    // [`bindings::__cpu_possible_mask`], the CPU index should begins from 0.
    PossibleCpusIndexIter { index: -1 }
}

/// Returns a [`OnlineCpusIndexIter`] that gives the online CPU indexes.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::cpumask::online_cpus;
///
/// fn example() {
///     // This prints all the online cpu indexes.
///     for cpu in online_cpus(){
///         pr_info!("{}\n", cpu.get());
///     }
/// }
/// ```
pub fn online_cpus() -> OnlineCpusIndexIter {
    // Initial index is set to -1. Since [`bindings::cpumask_next`] return the next set bit in a
    // [`bindings::__cpu_online_mask`], the CPU index should begins from 0.
    OnlineCpusIndexIter { index: -1 }
}

/// Returns a [`PresentCpusIndexIter`] that gives the present CPU indexes.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::cpumask::present_cpus;
///
/// fn example() {
///     // This prints all the present cpu indexes.
///     for cpu in present_cpus(){
///         pr_info!("{}\n", cpu.get());
///     }
/// }
/// ```
pub fn present_cpus() -> PresentCpusIndexIter {
    // Initial index is set to -1. Since [`bindings::cpumask_next`] return the next set bit in a
    // [`bindings::__cpu_present_mask`], the CPU index should begins from 0.
    PresentCpusIndexIter { index: -1 }
}
