// SPDX-License-Identifier: GPL-2.0

//! CPU utilities for SMP systems.
//!
//! C header: [`include/linux/smp.h`](../../../../include/linux/smp.h).
//!
//! Note: This module is a work in progress.

use crate::{bindings, error::code::*, prelude::*, to_result};
use core::{marker::PhantomData, ops::Deref};

/// Represents an SMP CPU by its zero-based kernel processor id.
///
/// [`Cpu`] values can be safely passed between threads (and across physical CPUs) as they are
/// merely an integer index representing a CPU. All actual interaction with CPUs is late-
/// bound.
#[derive(Copy, Clone)]
pub struct Cpu(u32);

struct FnCall<F, R>
where
    F: FnOnce() -> R,
{
    out: Option<R>,
    func: Option<F>,
}

unsafe extern "C" fn call_fn<F, R>(fn_box: *mut core::ffi::c_void)
where
    F: FnOnce() -> R,
{
    // SAFETY: The casting here is the inverse of the cast to `*mut c_void` that occurs in `Cpu::call`
    // below. That call waits for this call to complete ensuring `fn_call` is live for the duration.
    let fn_call = unsafe { &mut *(fn_box as *mut FnCall<F, R>) };
    let func = fn_call.func.take().unwrap();
    fn_call.out = Some(func());
}

impl Cpu {
    /// Constructs a new instance referring to a specific processor by id.
    ///
    /// This function checks that the passed `id` is a valid _potential_ processor id, but does
    /// not check that the processor is actually present or online. Manipulation of absent
    /// or offline processors will fail with reasonable errors returned.
    pub fn from_id(id: u32) -> Result<Cpu> {
        if id >= Self::num_possible() {
            return Err(E2BIG);
        }
        Ok(Cpu(id))
    }

    /// Returns the number of processors that are possible (including onlining and hotplug).
    pub fn num_possible() -> u32 {
        // SAFETY: The number of possible CPUs is fixed early at boot and does not change in
        // response to processors coming online, offline, or hotplug events.
        unsafe { bindings::nr_cpu_ids }
    }

    /// Lock the running thread to the current CPU and prevent preemption.
    pub fn lock_current() -> Guard {
        Guard::new()
    }

    /// Get the processor id of this CPU.
    pub fn id(&self) -> u32 {
        self.0
    }

    /// Call a function on the CPU identified by `self`.
    ///
    /// Take care when passing owned closures here as `drop` will be called in the IPI handler on
    /// the target CPU. If the closure captures expensive-to-destroy objects this may unnecessarily
    /// extend the time spent in the interrupt handler. If a closure captures any moved values
    /// (rather than simply references) please consider passing a `& impl Fn` or `&mut FnMut` for
    /// `func` to defer the `drop` until after the interrupt handler has completed.
    pub fn call<R, F>(&self, func: F) -> Result<R>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let mut fn_call = FnCall {
            out: None,
            func: Some(func),
        };
        // SAFETY: `smp_call_function_single` is invoked with `wait == 1` (true) ensuring that
        // `fn_call` outlives this FFI invocation. `func`'s returned value is moved into `fn_call.out`
        //  and then returned here. The `Send` type constraint ensures `func` and its returned value
        // can safely be moved between threads/CPUs.
        to_result(unsafe {
            bindings::smp_call_function_single(
                self.0 as i32,
                Some(call_fn::<F, R>),
                (&mut fn_call as *mut FnCall<F, R>).cast(),
                1,
            )
        })?;
        Ok(fn_call.out.unwrap())
    }
}

/// CPU guard preventing preemption of the current thread and keeping it locked to a single CPU.
pub struct Guard(
    // Cpu that this guard locks the current thread to.
    Cpu,
    // Wrap a pointer type to inhibit Rust's default `Sync` and `Send` trait impls.
    PhantomData<*mut ()>,
);

impl Guard {
    /// Lock the current thread to the current processor.
    fn new() -> Self {
        // SAFETY: FFI invocation; matching `put_cpu` occurs on `drop()`. `Guard` is neither `Sync` nor `Send`.
        let id = unsafe { bindings::get_cpu() };
        Self(Cpu(id as u32), PhantomData)
    }
}

impl Deref for Guard {
    type Target = Cpu;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: RAII companion to the `get_cpu()` from `Guard::new()`.
        unsafe { bindings::put_cpu() };
    }
}
