// SPDX-License-Identifier: GPL-2.0

//! Termios views and serial-core configuration under the UART lock.

use super::LockedPort;
use crate::bindings;
use core::num::NonZeroU32;

/// Termios borrowed for a serial-core set_termios callback.
#[repr(transparent)]
pub struct Termios(bindings::ktermios);

impl Termios {
    /// Control mode flags.
    pub fn cflag(&self) -> u32 {
        self.0.c_cflag
    }
    /// Replaces the control mode flags, including reflecting unsupported modes.
    pub fn set_cflag(&mut self, value: u32) {
        self.0.c_cflag = value;
    }
    /// Data bits per character.
    pub fn char_size(&self) -> u32 {
        // SAFETY: c_cflag is a valid termios control flag value.
        unsafe { bindings::tty_get_char_size(self.0.c_cflag).into() }
    }
}

impl LockedPort<'_> {
    /// Selects a supported baud rate using serial-core termios rules.
    pub fn baud_rate(&self, new: &mut Termios, old: Option<&Termios>, min: u32, max: u32) -> u32 {
        // SAFETY: The port is locked, and the termios references are valid for this callback.
        unsafe {
            bindings::uart_get_baud_rate(
                self.raw,
                &mut new.0,
                old.map_or(core::ptr::null(), |t| &t.0),
                min,
                max,
            )
        }
    }

    /// Updates serial-core timeout and the actual console baud rate.
    pub fn configure(&self, new: &mut Termios, baud: NonZeroU32) {
        let baud = baud.get();
        // SAFETY: All port fields are accessed under the UART lock. new is exclusively borrowed.
        unsafe {
            bindings::uart_update_timeout(self.raw, new.0.c_cflag, baud);
            if bindings::tty_termios_baud_rate(&new.0) != 0 {
                bindings::tty_termios_encode_baud_rate(&mut new.0, baud, baud);
            }
        }
    }
}
