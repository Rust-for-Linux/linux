// SPDX-License-Identifier: GPL-2.0-only

//! Transmission Control Protocol (TCP).

use crate::time;
use crate::types::Opaque;
use core::{num, ptr};

/// Representation of a `struct inet_connection_sock`.
///
/// # Invariants
///
/// Referencing a `inet_connection_sock` using this struct asserts that you are
/// in a context where all safe methods defined on this struct are indeed safe
/// to call.
///
/// C header: [`include/net/inet_connection_sock.h`](srctree/include/net/inet_connection_sock.h)
#[repr(transparent)]
pub struct InetConnectionSock {
    icsk: Opaque<bindings::inet_connection_sock>,
}

/// Representation of a `struct tcp_sock`.
///
/// # Invariants
///
/// Referencing a `tcp_sock` using this struct asserts that you are in
/// a context where all safe methods defined on this struct are indeed safe to
/// call.
///
/// C header: [`include/linux/tcp.h`](srctree/include/linux/tcp.h)
#[repr(transparent)]
pub struct TcpSock {
    tp: Opaque<bindings::tcp_sock>,
}

impl TcpSock {
    /// Returns true iff `snd_cwnd < snd_ssthresh`.
    #[inline]
    pub fn in_slow_start(&self) -> bool {
        // SAFETY: The struct invariant ensures that we may call this function
        // without additional synchronization.
        unsafe { bindings::tcp_in_slow_start(self.tp.get()) }
    }

    /// Performs the standard slow start increment of cwnd.
    ///
    /// If this causes the socket to exit slow start, any leftover ACKs are
    /// returned.
    #[inline]
    pub fn slow_start(&mut self, acked: u32) -> u32 {
        // SAFETY: The struct invariant ensures that we may call this function
        // without additional synchronization.
        unsafe { bindings::tcp_slow_start(self.tp.get(), acked) }
    }

    /// Performs the standard increase of cwnd during congestion avoidance.
    ///
    /// The increase per ACK is upper bounded by `1 / w`.
    #[inline]
    pub fn cong_avoid_ai(&mut self, w: num::NonZeroU32, acked: u32) {
        // SAFETY: The struct invariant ensures that we may call this function
        // without additional synchronization.
        unsafe { bindings::tcp_cong_avoid_ai(self.tp.get(), w.get(), acked) };
    }

    /// Returns the connection's current cwnd.
    #[inline]
    pub fn snd_cwnd(&self) -> u32 {
        // SAFETY: The struct invariant ensures that we may call this function
        // without additional synchronization.
        unsafe { bindings::tcp_snd_cwnd(self.tp.get()) }
    }

    /// Returns the connection's current ssthresh.
    #[inline]
    pub fn snd_ssthresh(&self) -> u32 {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of!((*self.tp.get()).snd_ssthresh) }
    }

    /// Returns the sequence number of the next byte that will be sent.
    #[inline]
    pub fn snd_nxt(&self) -> u32 {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of!((*self.tp.get()).snd_nxt) }
    }

    /// Returns the sequence number of the first unacknowledged byte.
    #[inline]
    pub fn snd_una(&self) -> u32 {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of!((*self.tp.get()).snd_una) }
    }

    /// Returns the time when the last packet was received or sent.
    #[inline]
    pub fn tcp_mstamp(&self) -> time::Usecs {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of!((*self.tp.get()).tcp_mstamp) }
    }

    /// Sets the connection's ssthresh.
    #[inline]
    pub fn set_snd_ssthresh(&mut self, new: u32) {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of_mut!((*self.tp.get()).snd_ssthresh) = new };
    }

    /// Returns the timestamp of the last send data packet in 32bit Jiffies.
    #[inline]
    pub fn lsndtime(&self) -> time::Jiffies32 {
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { *ptr::addr_of!((*self.tp.get()).lsndtime) as time::Jiffies32 }
    }
}

/// Tests if `sqn_1` comes after `sqn_2`.
#[inline]
pub fn after(sqn_1: u32, sqn_2: u32) -> bool {
    (sqn_2.wrapping_sub(sqn_1) as i32) < 0
}
