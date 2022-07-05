// SPDX-License-Identifier: GPL-2.0

//! Networking core.
//!
//! C headers: [`include/net/net_namespace.h`](../../../../include/linux/net/net_namespace.h),
//! [`include/linux/netdevice.h`](../../../../include/linux/netdevice.h),
//! [`include/linux/skbuff.h`](../../../../include/linux/skbuff.h).

use crate::{bindings, str::CStr, to_result, ARef, AlwaysRefCounted, Error, Result};
use core::{cell::UnsafeCell, ptr::NonNull};

#[cfg(CONFIG_NETFILTER)]
pub mod filter;

/// Wraps the kernel's `struct net_device`.
#[repr(transparent)]
pub struct Device(UnsafeCell<bindings::net_device>);

// SAFETY: Instances of `Device` are created on the C side. They are always refcounted.
unsafe impl AlwaysRefCounted for Device {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::dev_hold(self.0.get()) };
    }

    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::dev_put(obj.cast().as_ptr()) };
    }
}

/// Wraps the kernel's `struct net`.
#[repr(transparent)]
pub struct Namespace(UnsafeCell<bindings::net>);

impl Namespace {
    /// Finds a network device with the given name in the namespace.
    pub fn dev_get_by_name(&self, name: &CStr) -> Option<ARef<Device>> {
        // SAFETY: The existence of a shared reference guarantees the refcount is nonzero.
        let ptr =
            NonNull::new(unsafe { bindings::dev_get_by_name(self.0.get(), name.as_char_ptr()) })?;
        Some(unsafe { ARef::from_raw(ptr.cast()) })
    }
}

// SAFETY: Instances of `Namespace` are created on the C side. They are always refcounted.
unsafe impl AlwaysRefCounted for Namespace {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_net(self.0.get()) };
    }

    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::put_net(obj.cast().as_ptr()) };
    }
}

/// Returns the network namespace for the `init` process.
pub fn init_ns() -> &'static Namespace {
    unsafe { &*core::ptr::addr_of!(bindings::init_net).cast() }
}

/// Wraps the kernel's `struct sk_buff`.
#[repr(transparent)]
pub struct SkBuff(UnsafeCell<bindings::sk_buff>);

impl SkBuff {
    /// Creates a reference to an [`SkBuff`] from a valid pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `ptr` is valid and remains valid for the lifetime of the
    /// returned [`SkBuff`] instance.
    pub unsafe fn from_ptr<'a>(ptr: *const bindings::sk_buff) -> &'a SkBuff {
        // SAFETY: The safety requirements guarantee the validity of the dereference, while the
        // `SkBuff` type being transparent makes the cast ok.
        unsafe { &*ptr.cast() }
    }

    /// Returns the remaining data in the buffer's first segment.
    pub fn head_data(&self) -> &[u8] {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        let headlen = unsafe { bindings::skb_headlen(self.0.get()) };
        let len = headlen.try_into().unwrap_or(usize::MAX);
        // SAFETY: The existence of a shared reference means `self.0` is valid.
        let data = unsafe { core::ptr::addr_of!((*self.0.get()).data).read() };
        // SAFETY: The `struct sk_buff` conventions guarantee that at least `skb_headlen(skb)` bytes
        // are valid from `skb->data`.
        unsafe { core::slice::from_raw_parts(data, len) }
    }

    /// Returns the total length of the data (in all segments) in the skb.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u32 {
        // SAFETY: The existence of a shared reference means `self.0` is valid.
        unsafe { core::ptr::addr_of!((*self.0.get()).len).read() }
    }
}

// SAFETY: Instances of `SkBuff` are created on the C side. They are always refcounted.
unsafe impl AlwaysRefCounted for SkBuff {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::skb_get(self.0.get()) };
    }

    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe {
            bindings::kfree_skb_reason(
                obj.cast().as_ptr(),
                bindings::skb_drop_reason_SKB_DROP_REASON_NOT_SPECIFIED,
            )
        };
    }
}

/// An IPv4 address.
///
/// This is equivalent to C's `in_addr`.
#[repr(transparent)]
pub struct Ipv4Addr(bindings::in_addr);

impl Ipv4Addr {
    /// A wildcard IPv4 address.
    ///
    /// Binding to this address means binding to all IPv4 addresses.
    pub const ANY: Self = Self::new(0, 0, 0, 0);

    /// The IPv4 loopback address.
    pub const LOOPBACK: Self = Self::new(127, 0, 0, 1);

    /// The IPv4 broadcast address.
    pub const BROADCAST: Self = Self::new(255, 255, 255, 255);

    /// Creates a new IPv4 address with the given components.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self(bindings::in_addr {
            s_addr: u32::from_be_bytes([a, b, c, d]).to_be(),
        })
    }
}

/// An IPv6 address.
///
/// This is equivalent to C's `in6_addr`.
#[repr(transparent)]
pub struct Ipv6Addr(bindings::in6_addr);

impl Ipv6Addr {
    /// A wildcard IPv6 address.
    ///
    /// Binding to this address means binding to all IPv6 addresses.
    pub const ANY: Self = Self::new(0, 0, 0, 0, 0, 0, 0, 0);

    /// The IPv6 loopback address.
    pub const LOOPBACK: Self = Self::new(0, 0, 0, 0, 0, 0, 0, 1);

    /// Creates a new IPv6 address with the given components.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Self {
        Self(bindings::in6_addr {
            in6_u: bindings::in6_addr__bindgen_ty_1 {
                u6_addr16: [
                    a.to_be(),
                    b.to_be(),
                    c.to_be(),
                    d.to_be(),
                    e.to_be(),
                    f.to_be(),
                    g.to_be(),
                    h.to_be(),
                ],
            },
        })
    }
}

/// A socket address.
///
/// It's an enum with either an IPv4 or IPv6 socket address.
pub enum SocketAddr {
    /// An IPv4 socket address.
    V4(SocketAddrV4),

    /// An IPv6 socket address.
    V6(SocketAddrV6),
}

/// An IPv4 socket address.
///
/// This is equivalent to C's `sockaddr_in`.
#[repr(transparent)]
pub struct SocketAddrV4(bindings::sockaddr_in);

impl SocketAddrV4 {
    /// Creates a new IPv4 socket address.
    pub const fn new(addr: Ipv4Addr, port: u16) -> Self {
        Self(bindings::sockaddr_in {
            sin_family: bindings::AF_INET as _,
            sin_port: port.to_be(),
            sin_addr: addr.0,
            __pad: [0; 8],
        })
    }
}

/// An IPv6 socket address.
///
/// This is equivalent to C's `sockaddr_in6`.
#[repr(transparent)]
pub struct SocketAddrV6(bindings::sockaddr_in6);

impl SocketAddrV6 {
    /// Creates a new IPv6 socket address.
    pub const fn new(addr: Ipv6Addr, port: u16, flowinfo: u32, scopeid: u32) -> Self {
        Self(bindings::sockaddr_in6 {
            sin6_family: bindings::AF_INET6 as _,
            sin6_port: port.to_be(),
            sin6_addr: addr.0,
            sin6_flowinfo: flowinfo,
            sin6_scope_id: scopeid,
        })
    }
}

/// A socket listening on a TCP port.
///
/// # Invariants
///
/// The socket pointer is always non-null and valid.
pub struct TcpListener {
    pub(crate) sock: *mut bindings::socket,
}

// SAFETY: `TcpListener` is just a wrapper for a kernel socket, which can be used from any thread.
unsafe impl Send for TcpListener {}

// SAFETY: `TcpListener` is just a wrapper for a kernel socket, which can be used from any thread.
unsafe impl Sync for TcpListener {}

impl TcpListener {
    /// Creates a new TCP listener.
    ///
    /// It is configured to listen on the given socket address for the given namespace.
    pub fn try_new(ns: &Namespace, addr: &SocketAddr) -> Result<Self> {
        let mut socket = core::ptr::null_mut();
        let (pf, addr, addrlen) = match addr {
            SocketAddr::V4(addr) => (
                bindings::PF_INET,
                addr as *const _ as _,
                core::mem::size_of::<bindings::sockaddr_in>(),
            ),
            SocketAddr::V6(addr) => (
                bindings::PF_INET6,
                addr as *const _ as _,
                core::mem::size_of::<bindings::sockaddr_in6>(),
            ),
        };

        // SAFETY: The namespace is valid and the output socket pointer is valid for write.
        to_result(unsafe {
            bindings::sock_create_kern(
                ns.0.get(),
                pf as _,
                bindings::sock_type_SOCK_STREAM as _,
                bindings::IPPROTO_TCP as _,
                &mut socket,
            )
        })?;

        // INVARIANT: The socket was just created, so it is valid.
        let listener = Self { sock: socket };

        // SAFETY: The type invariant guarantees that the socket is valid, and `addr` and `addrlen`
        // were initialised based on valid values provided in the address enum.
        to_result(unsafe { bindings::kernel_bind(socket, addr, addrlen as _) })?;

        // SAFETY: The socket is valid per the type invariant.
        to_result(unsafe { bindings::kernel_listen(socket, bindings::SOMAXCONN as _) })?;

        Ok(listener)
    }

    /// Accepts a new connection.
    ///
    /// On success, returns the newly-accepted socket stream.
    ///
    /// If no connection is available to be accepted, one of two behaviours will occur:
    /// - If `block` is `false`, returns [`crate::error::code::EAGAIN`];
    /// - If `block` is `true`, blocks until an error occurs or some connection can be accepted.
    pub fn accept(&self, block: bool) -> Result<TcpStream> {
        let mut new = core::ptr::null_mut();
        let flags = if block { 0 } else { bindings::O_NONBLOCK };
        // SAFETY: The type invariant guarantees that the socket is valid, and the output argument
        // is also valid for write.
        to_result(unsafe { bindings::kernel_accept(self.sock, &mut new, flags as _) })?;
        Ok(TcpStream { sock: new })
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        // SAFETY: The type invariant guarantees that the socket is valid.
        unsafe { bindings::sock_release(self.sock) };
    }
}

/// A connected TCP socket.
///
/// # Invariants
///
/// The socket pointer is always non-null and valid.
pub struct TcpStream {
    pub(crate) sock: *mut bindings::socket,
}

// SAFETY: `TcpStream` is just a wrapper for a kernel socket, which can be used from any thread.
unsafe impl Send for TcpStream {}

// SAFETY: `TcpStream` is just a wrapper for a kernel socket, which can be used from any thread.
unsafe impl Sync for TcpStream {}

impl TcpStream {
    /// Reads data from a connected socket.
    ///
    /// On success, returns the number of bytes read, which will be zero if the connection is
    /// closed.
    ///
    /// If no data is immediately available for reading, one of two behaviours will occur:
    /// - If `block` is `false`, returns [`crate::error::code::EAGAIN`];
    /// - If `block` is `true`, blocks until an error occurs, the connection is closed, or some
    ///   becomes readable.
    pub fn read(&self, buf: &mut [u8], block: bool) -> Result<usize> {
        let mut msg = bindings::msghdr::default();
        let mut vec = bindings::kvec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: The type invariant guarantees that the socket is valid, and `vec` was
        // initialised with the output buffer.
        let r = unsafe {
            bindings::kernel_recvmsg(
                self.sock,
                &mut msg,
                &mut vec,
                1,
                vec.iov_len,
                if block { 0 } else { bindings::MSG_DONTWAIT } as _,
            )
        };
        if r < 0 {
            Err(Error::from_kernel_errno(r))
        } else {
            Ok(r as _)
        }
    }

    /// Writes data to the connected socket.
    ///
    /// On success, returns the number of bytes written.
    ///
    /// If the send buffer of the socket is full, one of two behaviours will occur:
    /// - If `block` is `false`, returns [`crate::error::code::EAGAIN`];
    /// - If `block` is `true`, blocks until an error occurs or some data is written.
    pub fn write(&self, buf: &[u8], block: bool) -> Result<usize> {
        let mut msg = bindings::msghdr {
            msg_flags: if block { 0 } else { bindings::MSG_DONTWAIT },
            ..bindings::msghdr::default()
        };
        let mut vec = bindings::kvec {
            iov_base: buf.as_ptr() as *mut u8 as _,
            iov_len: buf.len(),
        };
        // SAFETY: The type invariant guarantees that the socket is valid, and `vec` was
        // initialised with the input  buffer.
        let r = unsafe { bindings::kernel_sendmsg(self.sock, &mut msg, &mut vec, 1, vec.iov_len) };
        if r < 0 {
            Err(Error::from_kernel_errno(r))
        } else {
            Ok(r as _)
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        // SAFETY: The type invariant guarantees that the socket is valid.
        unsafe { bindings::sock_release(self.sock) };
    }
}
