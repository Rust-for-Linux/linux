// SPDX-License-Identifier: GPL-2.0

// Copyright (C) 2024 Google LLC.

//! Logic for tracepoints.

/// Declare the Rust entry point for a tracepoint.
#[macro_export]
macro_rules! declare_trace {
    ($($(#[$attr:meta])* $pub:vis fn $name:ident($($argname:ident : $argtyp:ty),* $(,)?);)*) => {$(
        $( #[$attr] )*
        #[inline(always)]
        $pub unsafe fn $name($($argname : $argtyp),*) {
            #[cfg(CONFIG_TRACEPOINTS)]
            {
                use $crate::bindings::*;

                // SAFETY: This macro only compiles if $name is a real tracepoint, and if it is a
                // real tracepoint, then it is okay to query the static key.
                let should_trace = unsafe {
                    $crate::macros::paste! {
                        $crate::static_key::static_key_false!(
                            [< __tracepoint_ $name >],
                            $crate::bindings::tracepoint,
                            key
                        )
                    }
                };

                if should_trace {
                    // TODO: cpu_online(raw_smp_processor_id())
                    let cond = true;
                    $crate::tracepoint::do_trace!($name($($argname : $argtyp),*), cond);
                }
            }

            #[cfg(not(CONFIG_TRACEPOINTS))]
            {
                // If tracepoints are disabled, insert a trivial use of each argument
                // to avoid unused argument warnings.
                $( let _unused = $argname; )*
            }
        }
    )*}
}

#[doc(hidden)]
#[macro_export]
macro_rules! do_trace {
    ($name:ident($($argname:ident : $argtyp:ty),* $(,)?), $cond:expr) => {{
        if !$cond {
            return;
        }

        // SAFETY: This call is balanced with the call below.
        unsafe { $crate::bindings::preempt_disable_notrace() };

        // SAFETY: This calls the tracepoint with the provided arguments. The caller of the Rust
        // wrapper guarantees that this is okay.
        #[cfg(CONFIG_HAVE_STATIC_CALL)]
        unsafe {
            let it_func_ptr: *mut $crate::bindings::tracepoint_func =
                $crate::bindings::rcu_dereference_raw(
                    ::core::ptr::addr_of!(
                        $crate::macros::concat_idents!(__tracepoint_, $name).funcs
                    )
                );

            if !it_func_ptr.is_null() {
                let __data = (*it_func_ptr).data;
                $crate::macros::paste! {
                    $crate::static_call::static_call! {
                        [< tp_func_ $name >] (__data, $($argname),*)
                    };
                }
            }
        }

        // SAFETY: This calls the tracepoint with the provided arguments. The caller of the Rust
        // wrapper guarantees that this is okay.
        #[cfg(not(CONFIG_HAVE_STATIC_CALL))]
        unsafe {
            $crate::macros::concat_idents!(__traceiter_, $name)(
                ::core::ptr::null_mut(),
                $($argname),*
            );
        }

        // SAFETY: This call is balanced with the call above.
        unsafe { $crate::bindings::preempt_enable_notrace() };
    }}
}

pub use {declare_trace, do_trace};
