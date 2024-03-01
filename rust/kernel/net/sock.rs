// SPDX-License-Identifier: GPL-2.0-only

//! Representation of a C `struct sock`.
//!
//! C header: [`include/net/sock.h`](srctree/include/net/sock.h)

#[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
use crate::net::tcp::{self, InetConnectionSock, TcpSock};
use crate::types::Opaque;
use core::convert::TryFrom;
use core::ptr::addr_of;

/// Representation of a C `struct sock`.
///
/// Not intended to be used directly by modules. Abstractions should provide a
/// safe interface to only those operations that are OK to use for the module.
///
/// # Invariants
///
/// Referencing a `sock` using this struct asserts that you are in
/// a context where all safe methods defined on this struct are indeed safe to
/// call.
#[repr(transparent)]
pub(crate) struct Sock {
    sk: Opaque<bindings::sock>,
}

impl Sock {
    /// Returns a raw pointer to the wrapped `struct sock`.
    ///
    /// It is up to the caller to use it correctly.
    #[inline]
    pub(crate) fn raw_sk_mut(&mut self) -> *mut bindings::sock {
        self.sk.get()
    }

    /// Returns the sockets pacing rate in bytes per second.
    #[inline]
    pub(crate) fn sk_pacing_rate(&self) -> u64 {
        // NOTE: C uses READ_ONCE for this field, thus `read_volatile`.
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization. It is a C unsigned
        // long so we can always convert it to a u64 without loss.
        unsafe { addr_of!((*self.sk.get()).sk_pacing_rate).read_volatile() as u64 }
    }

    /// Returns the sockets pacing status.
    #[inline]
    pub(crate) fn sk_pacing_status(&self) -> Result<Pacing, ()> {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { Pacing::try_from(*addr_of!((*self.sk.get()).sk_pacing_status)) }
    }

    /// Returns the sockets maximum GSO segment size to build.
    #[inline]
    pub(crate) fn sk_gso_max_size(&self) -> u32 {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization. It is an unsigned int
        // and we are guaranteed that this will always fit into a u32.
        unsafe { *addr_of!((*self.sk.get()).sk_gso_max_size) as u32 }
    }

    /// Returns the [`TcpSock`] that is containing the `Sock`.
    ///
    /// # Safety
    ///
    /// `sk` must be valid for `tcp_sk`.
    #[inline]
    #[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
    pub(crate) unsafe fn tcp_sk<'a>(&'a self) -> &'a TcpSock {
        // SAFETY:
        // - Downcasting via `tcp_sk` is OK by the functions precondition.
        // - The cast is OK since `TcpSock` is transparent to `struct tcp_sock`.
        unsafe { &*(bindings::tcp_sk(self.sk.get()) as *const TcpSock) }
    }

    /// Returns the [`TcpSock`] that is containing the `Sock`.
    ///
    /// # Safety
    ///
    /// `sk` must be valid for `tcp_sk`.
    #[inline]
    #[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
    pub(crate) unsafe fn tcp_sk_mut<'a>(&'a mut self) -> &'a mut TcpSock {
        // SAFETY:
        // - Downcasting via `tcp_sk` is OK by the functions precondition.
        // - The cast is OK since `TcpSock` is transparent to `struct tcp_sock`.
        unsafe { &mut *(bindings::tcp_sk(self.sk.get()) as *mut TcpSock) }
    }

    /// Returns the [`InetConnectionSock`] view of this socket.
    ///
    /// # Safety
    ///
    /// `sk` must be valid for `inet_csk`.
    #[inline]
    #[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
    pub(crate) unsafe fn inet_csk<'a>(&'a self) -> &'a InetConnectionSock {
        // SAFETY:
        // - Calling `inet_csk` is OK by the functions precondition.
        // - The cast is OK since `InetConnectionSock` is transparent to
        //   `struct inet_connection_sock`.
        unsafe { &*(bindings::inet_csk(self.sk.get()) as *const InetConnectionSock) }
    }

    /// Tests if the connection's sending rate is limited by the cwnd.
    ///
    /// # Safety
    ///
    /// `sk` must be valid for `tcp_is_cwnd_limited`.
    #[inline]
    #[cfg(CONFIG_RUST_TCP_ABSTRACTIONS)]
    pub(crate) unsafe fn tcp_is_cwnd_limited(&self) -> bool {
        // SAFETY: Calling `tcp_is_cwnd_limited` is OK by the functions
        // precondition.
        unsafe { bindings::tcp_is_cwnd_limited(self.sk.get()) }
    }
}

/// The socket's pacing status.
#[repr(u32)]
#[allow(missing_docs)]
pub enum Pacing {
    r#None = bindings::sk_pacing_SK_PACING_NONE,
    Needed = bindings::sk_pacing_SK_PACING_NEEDED,
    Fq = bindings::sk_pacing_SK_PACING_FQ,
}

// TODO: Replace with automatically generated code by bindgen when it becomes
// possible.
impl TryFrom<u32> for Pacing {
    type Error = ();

    fn try_from(val: u32) -> Result<Self, Self::Error> {
        match val {
            x if x == Pacing::r#None as u32 => Ok(Pacing::r#None),
            x if x == Pacing::Needed as u32 => Ok(Pacing::Needed),
            x if x == Pacing::Fq as u32 => Ok(Pacing::Fq),
            _ => Err(()),
        }
    }
}
