// SPDX-License-Identifier: GPL-2.0

//! Networking.

#[cfg(CONFIG_RUST_PHYLIB_ABSTRACTIONS)]
pub mod phy;
#[cfg(CONFIG_RUST_SOCK_ABSTRACTIONS)]
pub mod sock;
#[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
pub mod tcp;
