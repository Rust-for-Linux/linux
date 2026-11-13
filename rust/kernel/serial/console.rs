// SPDX-License-Identifier: GPL-2.0

//! Legacy polling console callbacks and emergency lock handling.

use super::{Port, PortData, PortGuard, SerialOps};
use crate::{bindings, prelude::*};
use core::marker::PhantomData;

/// Converts legacy console callbacks without owning a separate registration.
pub(super) struct ConsoleAdapter<T: SerialOps>(PhantomData<T>);

impl<T: SerialOps> ConsoleAdapter<T> {
    unsafe fn console_port(console: *mut bindings::console) -> *mut bindings::uart_port {
        // SAFETY: Serial core invokes callbacks only while this pinned console and driver
        // are live. Port removal unregisters the console before releasing its port state.
        unsafe {
            let driver = (*console).data.cast::<bindings::uart_driver>();
            if driver.is_null() || (*driver).state.is_null() {
                return core::ptr::null_mut();
            }
            let index = (*console).index;
            if index < 0 || i32::from(index) >= (*driver).nr {
                return core::ptr::null_mut();
            }
            (*(*driver).state.add(index as usize)).uart_port
        }
    }

    pub(super) unsafe extern "C" fn console_setup(
        console: *mut bindings::console,
        options: *mut crate::ffi::c_char,
    ) -> i32 {
        // SAFETY: Console core holds console_list_lock; serial core has published the port
        // before registering this console. uart_set_options calls our termios adapter.
        unsafe {
            let port = Self::console_port(console);
            if port.is_null() {
                return ENODEV.to_errno();
            }
            let mut baud = (*console).ispeed as i32;
            let (mut parity, mut bits, mut flow) = (b'n' as i32, 8, b'n' as i32);
            if !options.is_null() {
                bindings::uart_parse_options(options, &mut baud, &mut parity, &mut bits, &mut flow);
            }
            bindings::uart_set_options(port, console, baud, parity, bits, flow)
        }
    }

    pub(super) unsafe extern "C" fn console_write(
        console: *mut bindings::console,
        text: *const crate::ffi::c_char,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        // SAFETY: Legacy console writes hold console_srcu. unregister_console() waits
        // for these readers before Registration releases the pinned port and data.
        // Driver<T> only registers PortData<T>, so private_data has the matching type.
        unsafe {
            let port = Self::console_port(console);
            if !port.is_null() {
                PortData::<T>::with_port(port, |state, port| {
                    let bytes = core::slice::from_raw_parts(text.cast(), count as usize);
                    port.with_console_access(|| state.data.console_write(port, bytes));
                });
            }
        }
    }
}

impl Port<'_> {
    /// Runs bounded, non-sleeping console I/O, including during emergency output.
    ///
    /// Normally the UART lock is held with local IRQs saved. During an oops a
    /// failed trylock runs `f` without that lock: MMIO may race and output is
    /// best effort. No LockedPort is provided, so locked serial-core state must
    /// not be accessed here. Rust data shared by callbacks must remain Sync.
    pub(super) fn with_console_access<R>(&self, f: impl FnOnce() -> R) -> R {
        let mut flags = 0;
        // SAFETY: The port is live throughout the console callback.
        let unlock = unsafe { bindings::uart_port_console_lock(self.raw, &mut flags) };
        let guard = PortGuard {
            port: self.raw,
            flags,
            unlock,
        };
        let result = f();
        drop(guard);
        result
    }
}
