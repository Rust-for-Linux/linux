// SPDX-License-Identifier: GPL-2.0

//! Networking core.
//!
//! C headers: [`include/net/net_namespace.h`](../../../../include/linux/net/net_namespace.h),
//! [`include/linux/netdevice.h`](../../../../include/linux/netdevice.h),
//! [`include/linux/skbuff.h`](../../../../include/linux/skbuff.h).
//! [`include/linux/ethtool.h`](../../../../include/linux/ethtool.h).

use crate::{
    bindings,
    error::{from_kernel_err_ptr, from_kernel_result},
    prelude::*,
    str::CStr,
    to_result, ARef, AlwaysRefCounted, Error, PointerWrapper, Result,
};
use alloc::slice;
use core::{self, cell::UnsafeCell, ptr::NonNull};

#[cfg(CONFIG_NETFILTER)]
pub mod filter;

/// Wraps the kernel's `struct net_device`.
#[repr(transparent)]
pub struct Device(UnsafeCell<bindings::net_device>);

impl Device {
    pub fn alloc_etherdev_mqs(sizeof: i32, count: u32) -> Result<ARef<Self>> {
        // SAFETY: FFI call.
        let res =
            from_kernel_err_ptr(unsafe { bindings::alloc_etherdev_mqs(sizeof, count, count) })?;
        // SAFETY: Since the `net_device` creation succeeded, the `res` must be valid.
        let net: ARef<_> = unsafe { &*(res as *const Device) }.into();
        Ok(net)
    }
}

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

    /// Creates a new IPv4 socket address from C's `sockaddr_in` or `sockaddr` pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `ptr` points to C's `sockaddr_in` or `sockaddr`.
    pub const unsafe fn from_ptr(ptr: *mut core::ffi::c_void) -> Self {
        // Safety: The safety requirements guarantee the validity of the cast.
        let ptr_n = ptr as *mut bindings::sockaddr_in;
        // SAFETY: The above cast guarantees the validity of the dereference.
        unsafe {
            Self(bindings::sockaddr_in {
                sin_family: (*ptr_n).sin_family,
                sin_port: (*ptr_n).sin_port,
                sin_addr: (*ptr_n).sin_addr,
                __pad: (*ptr_n).__pad,
            })
        }
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

/// A structure for `NAPI` scheduling with weighting.
pub struct NapiStruct {
    napi: bindings::napi_struct,
}

impl NapiStruct {
    /// New an uninitialized `NapiStruct`.
    ///
    /// # Safety
    ///
    /// Callers must call [`NapiStruct::init`] before using the `NapiStruct` item.
    unsafe fn new() -> Self {
        Self {
            napi: bindings::napi_struct::default(),
        }
    }

    // Init the `NapiStruct` item.
    fn init(
        &mut self,
        dev: &mut Device,
        poll: Option<
            unsafe extern "C" fn(
                arg1: *mut bindings::napi_struct,
                arg2: core::ffi::c_int,
            ) -> core::ffi::c_int,
        >,
        weight: i32,
    ) -> Result<usize> {
        // SAFETY: The existence of the shared references mean `dev.0` and `self.0` are valid.
        unsafe {
            bindings::netif_napi_add_weight(
                dev.0.get_mut() as _,
                &mut self.napi as *mut bindings::napi_struct,
                poll,
                weight,
            );
        }
        Ok(0)
    }
}

/// The main device statistics structure.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct RtnlLinkStats64 {
    ptr: *mut bindings::rtnl_link_stats64,
}

impl RtnlLinkStats64 {
    /// Constructs a new `struct rtnl_link_stats64` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::rtnl_link_stats64) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// RX/TX ring parameters.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolRingparam {
    ptr: *mut bindings::ethtool_ringparam,
}

impl EthtoolRingparam {
    /// Constructs a new `struct ethtool_ringparam` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_ringparam) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }

    /// Sets the rx_max_pending associated with the ethtool ring param.
    pub fn set_rx_max_pending(&mut self, rx_max_pending: u32) {
        // SAFETY: `self.ptr` is valid by the type invariants.
        unsafe { (*self.ptr).rx_max_pending = rx_max_pending as _ };
    }

    /// Sets the tx_max_pending associated with the ethtool ring param.
    pub fn set_tx_max_pending(&mut self, tx_max_pending: u32) {
        // SAFETY: `self.ptr` is valid by the type invariants.
        unsafe { (*self.ptr).tx_max_pending = tx_max_pending as _ };
    }

    /// Sets the rx_pending associated with the ethtool ring param.
    pub fn set_rx_pending(&mut self, rx_pending: u32) {
        // SAFETY: `self.ptr` is valid by the type invariants.
        unsafe { (*self.ptr).rx_pending = rx_pending as _ };
    }

    /// Sets the tx_pending associated with the ethtool ring param.
    pub fn set_tx_pending(&mut self, tx_pending: u32) {
        // SAFETY: `self.ptr` is valid by the type invariants.
        unsafe { (*self.ptr).tx_pending = tx_pending as _ };
    }
}

/// General driver and device information.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolDrvinfo {
    ptr: *mut bindings::ethtool_drvinfo,
}

impl EthtoolDrvinfo {
    /// Constructs a new `struct irq_domain` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_drvinfo) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Device-specific statistics.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolStats {
    ptr: *mut bindings::ethtool_stats,
}

impl EthtoolStats {
    /// Constructs a new `struct ethtool_stats` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_stats) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Configuring number of network channel.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolChannels {
    ptr: *mut bindings::ethtool_channels,
}

impl EthtoolChannels {
    /// Constructs a new `struct ethtool_channels` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_channels) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Link control and status.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolLinkKsettings {
    ptr: *const bindings::ethtool_link_ksettings,
}

impl EthtoolLinkKsettings {
    /// Constructs a new `struct ethtool_link_ksettings` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *const bindings::ethtool_link_ksettings) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Command to get or set RX flow classification rules.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolRxnfc {
    ptr: *mut bindings::ethtool_rxnfc,
}

impl EthtoolRxnfc {
    /// Constructs a new `struct ethtool_rxnfc` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_rxnfc) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Coalescing parameters for IRQs and stats updates.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolCoalesce {
    ptr: *mut bindings::ethtool_coalesce,
}

impl EthtoolCoalesce {
    /// Constructs a new `struct ethtool_coalesce` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_coalesce) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

/// Holds a device's timestamping and PHC association.
///
/// # Invariants
///
/// `ptr` is always non-null and valid.
pub struct EthtoolTsInfo {
    ptr: *mut bindings::ethtool_ts_info,
}

impl EthtoolTsInfo {
    /// Constructs a new `struct ethtool_ts_info` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::ethtool_ts_info) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }
}

#[vtable]
pub trait EthtoolOps {
    /// The pointer type that will be used to hold user-defined data type.
    type DataEthtoolOps: PointerWrapper + Send + Sync = ();

    fn get_drvinfo(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        info: EthtoolDrvinfo,
    );

    fn get_link(data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>) -> u32;

    fn get_ringparam(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ring: EthtoolRingparam,
    );

    fn get_strings(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        stringset: u32,
        ptr: *mut u8,
    ) -> Result<u32>;

    fn get_sset_count(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        sset: i32,
    ) -> Result<i32>;

    fn get_ethtool_stats(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        stats: EthtoolStats,
        data: *mut u64,
    );

    fn set_channels(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_channels: EthtoolChannels,
    ) -> Result<u32>;

    fn get_channels(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_channels: EthtoolChannels,
    );

    fn get_ts_info(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        info: EthtoolTsInfo,
    ) -> Result<u32>;

    fn get_link_ksettings(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_link_ksettings: EthtoolLinkKsettings,
    ) -> Result<u32>;

    fn set_link_ksettings(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_link_ksettings: EthtoolLinkKsettings,
    ) -> Result<u32>;

    fn set_coalesce(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_coalesce: EthtoolCoalesce,
    ) -> Result<i32>;

    fn get_coalesce(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        ethtool_coalesce: EthtoolCoalesce,
    ) -> Result<i32>;

    fn get_rxfh_key_size(data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>) -> u32;

    fn get_rxfh_indir_size(data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>) -> u32;

    fn get_rxfh(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        indir: *mut u32,
        key: *mut u8,
        hfunc: *mut u8,
    ) -> Result<i32>;

    fn set_rxfh(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        indir: *const u32,
        key: *const u8,
        hfunc: u8,
    ) -> Result<i32>;

    fn get_rxnfc(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        info: EthtoolRxnfc,
    ) -> Result<u32>;

    fn set_rxnfc(
        data: <Self::DataEthtoolOps as PointerWrapper>::Borrowed<'_>,
        info: EthtoolRxnfc,
    ) -> Result<u32>;
}

#[vtable]
pub trait NetdevOps {
    /// The pointer type that will be used to hold user-defined data type.
    type DataNetdevOps: PointerWrapper + Send + Sync = ();

    fn ndo_open(data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>) -> Result<i32>;

    fn ndo_stop(data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>) -> Result<i32>;

    fn ndo_start_xmit(
        skb: &SkBuff,
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
    ) -> bindings::netdev_tx_t;

    fn ndo_validate_addr(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
    ) -> Result<i32>;

    fn ndo_set_mac_address(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        addr: SocketAddr,
    ) -> Result<i32>;

    fn ndo_set_rx_mode(data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>);

    fn ndo_get_stats64(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        storage: RtnlLinkStats64,
    );

    fn ndo_vlan_rx_add_vid(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        proto: u16,
        vid: u16,
    ) -> Result<i32>;

    fn ndo_vlan_rx_kill_vid(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        proto: u16,
        vid: u16,
    ) -> Result<i32>;

    fn ndo_bpf(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        buffer: &mut [u8],
    ) -> Result<u32>;

    fn ndo_xdp_xmit(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        buffer: &mut [u8],
    ) -> Result<u32>;

    fn ndo_features_check(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        skb: &SkBuff,
        info: bindings::netdev_features_t,
    ) -> bindings::netdev_features_t;

    fn ndo_get_phys_port_name(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        buffer: &mut [u8],
    ) -> Result<i32>;

    fn ndo_set_features(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        features: bindings::netdev_features_t,
    ) -> Result<i32>;

    fn ndo_tx_timeout(
        data: <Self::DataNetdevOps as PointerWrapper>::Borrowed<'_>,
        txqueue: core::ffi::c_uint,
    );
}

struct NetDeviceTables<T: EthtoolOps + NetdevOps>(T);
impl<T: EthtoolOps + NetdevOps> NetDeviceTables<T> {
    const ETHTOOL_OPS: bindings::ethtool_ops = bindings::ethtool_ops {
        _bitfield_1: bindings::__BindgenBitfieldUnit::new([1; 1]),
        supported_ring_params: 0,
        supported_coalesce_params: 0,
        get_drvinfo: if <T>::HAS_GET_DRVINFO {
            Some(Self::get_drvinfo_callback)
        } else {
            None
        },
        get_link: if <T>::HAS_GET_LINK {
            Some(Self::get_link_callback)
        } else {
            None
        },
        get_ringparam: if <T>::HAS_GET_RINGPARAM {
            Some(Self::get_ringparam_callback)
        } else {
            None
        },
        get_strings: if <T>::HAS_GET_STRINGS {
            Some(Self::get_strings_callback)
        } else {
            None
        },
        get_sset_count: if <T>::HAS_GET_SSET_COUNT {
            Some(Self::get_sset_count_callback)
        } else {
            None
        },
        get_ethtool_stats: if <T>::HAS_GET_ETHTOOL_STATS {
            Some(Self::get_ethtool_stats_callback)
        } else {
            None
        },
        set_channels: if <T>::HAS_SET_CHANNELS {
            Some(Self::set_channels_callback)
        } else {
            None
        },
        get_channels: if <T>::HAS_GET_CHANNELS {
            Some(Self::get_channels_callback)
        } else {
            None
        },
        get_ts_info: if <T>::HAS_GET_TS_INFO {
            Some(Self::get_ts_info_callback)
        } else {
            None
        },
        get_link_ksettings: if <T>::HAS_GET_LINK_KSETTINGS {
            Some(Self::get_link_ksettings_callback)
        } else {
            None
        },
        set_link_ksettings: if <T>::HAS_SET_LINK_KSETTINGS {
            Some(Self::set_link_ksettings_callback)
        } else {
            None
        },
        set_coalesce: if <T>::HAS_SET_COALESCE {
            Some(Self::set_coalesce_callback)
        } else {
            None
        },
        get_coalesce: if <T>::HAS_GET_COALESCE {
            Some(Self::get_coalesce_callback)
        } else {
            None
        },
        get_rxfh_key_size: if <T>::HAS_GET_RXFH_KEY_SIZE {
            Some(Self::get_rxfh_key_size_callback)
        } else {
            None
        },
        get_rxfh_indir_size: if <T>::HAS_GET_RXFH_INDIR_SIZE {
            Some(Self::get_rxfh_indir_size_callback)
        } else {
            None
        },
        get_rxfh: if <T>::HAS_GET_RXFH {
            Some(Self::get_rxfh_callback)
        } else {
            None
        },
        set_rxfh: if <T>::HAS_SET_RXFH {
            Some(Self::set_rxfh_callback)
        } else {
            None
        },
        get_rxnfc: if <T>::HAS_GET_RXNFC {
            Some(Self::get_rxnfc_callback)
        } else {
            None
        },
        set_rxnfc: if <T>::HAS_SET_RXNFC {
            Some(Self::set_rxnfc_callback)
        } else {
            None
        },
        get_regs_len: None,
        get_regs: None,
        get_wol: None,
        set_wol: None,
        get_msglevel: None,
        set_msglevel: None,
        nway_reset: None,
        get_link_ext_state: None,
        get_eeprom_len: None,
        get_eeprom: None,
        set_eeprom: None,
        set_ringparam: None,
        get_pause_stats: None,
        get_pauseparam: None,
        set_pauseparam: None,
        self_test: None,
        set_phys_id: None,
        begin: None,
        complete: None,
        get_priv_flags: None,
        set_priv_flags: None,
        flash_device: None,
        reset: None,
        get_rxfh_context: None,
        set_rxfh_context: None,
        get_dump_flag: None,
        get_dump_data: None,
        set_dump: None,
        get_module_info: None,
        get_module_eeprom: None,
        get_eee: None,
        set_eee: None,
        get_tunable: None,
        set_tunable: None,
        get_per_queue_coalesce: None,
        set_per_queue_coalesce: None,
        get_fec_stats: None,
        get_fecparam: None,
        set_fecparam: None,
        get_ethtool_phy_stats: None,
        get_phy_tunable: None,
        set_phy_tunable: None,
        get_module_eeprom_by_page: None,
        get_eth_phy_stats: None,
        get_eth_mac_stats: None,
        get_eth_ctrl_stats: None,
        get_rmon_stats: None,
        get_module_power_mode: None,
        set_module_power_mode: None,
    };

    unsafe extern "C" fn get_drvinfo_callback(
        dev: *mut bindings::net_device,
        info: *mut bindings::ethtool_drvinfo,
    ) {
        // SAFETY: `dev` is valid as it was passed in by the C portion.
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
        // function call.
        let driver_info = unsafe { EthtoolDrvinfo::from_ptr(info) };
        T::get_drvinfo(data, driver_info);
    }

    unsafe extern "C" fn get_link_callback(dev: *mut bindings::net_device) -> bindings::u32_ {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        let res = T::get_link(data);
        res
    }

    unsafe extern "C" fn get_ringparam_callback(
        dev: *mut bindings::net_device,
        ring: *mut bindings::ethtool_ringparam,
        kernel_ring: *mut bindings::kernel_ethtool_ringparam,
        extack: *mut bindings::netlink_ext_ack,
    ) {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `ring` remains valid for the duration of this
        // function call.
        let ring = unsafe { EthtoolRingparam::from_ptr(ring) };
        T::get_ringparam(data, ring);
    }

    unsafe extern "C" fn get_strings_callback(
        dev: *mut bindings::net_device,
        stringset: u32,
        ptr: *mut u8,
    ) {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        T::get_strings(data, stringset, ptr);
    }

    unsafe extern "C" fn get_sset_count_callback(
        dev: *mut bindings::net_device,
        sset: i32,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::get_sset_count(data, sset)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_ethtool_stats_callback(
        dev: *mut bindings::net_device,
        stats: *mut bindings::ethtool_stats,
        ptr: *mut u64,
    ) {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
        // function call.
        let ethtool_stats = unsafe { EthtoolStats::from_ptr(stats) };
        T::get_ethtool_stats(data, ethtool_stats, ptr);
    }

    unsafe extern "C" fn set_channels_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_channels,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let ethtool_channels = unsafe{EthtoolChannels::from_ptr(ptr)};
            let res = T::set_channels(data, ethtool_channels)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_channels_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_channels,
    ) {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
        // function call.
        let ethtool_channels = unsafe { EthtoolChannels::from_ptr(ptr) };
        T::get_channels(data, ethtool_channels);
    }

    unsafe extern "C" fn get_ts_info_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_ts_info,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let info =  unsafe{EthtoolTsInfo::from_ptr(ptr)};
            let res = T::get_ts_info(data, info)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_link_ksettings_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_link_ksettings,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let cmd = unsafe{EthtoolLinkKsettings::from_ptr(ptr)};
            let res = T::get_link_ksettings(data, cmd)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn set_link_ksettings_callback(
        dev: *mut bindings::net_device,
        ptr: *const bindings::ethtool_link_ksettings,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let cmd = unsafe{EthtoolLinkKsettings::from_ptr(ptr)};
            let res = T::set_link_ksettings(data, cmd)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn set_coalesce_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_coalesce,
        kernel_coal: *mut bindings::kernel_ethtool_coalesce,
        extack: *mut bindings::netlink_ext_ack,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let ethtool_coalesce = unsafe{EthtoolCoalesce::from_ptr(ptr)};
            let res = T::set_coalesce(data, ethtool_coalesce)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_coalesce_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_coalesce,
        kernel_coal: *mut bindings::kernel_ethtool_coalesce,
        extack: *mut bindings::netlink_ext_ack,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let ethtool_coalesce = unsafe{EthtoolCoalesce::from_ptr(ptr)};
            let res = T::get_coalesce(data, ethtool_coalesce)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_rxfh_key_size_callback(
        dev: *mut bindings::net_device,
    ) -> bindings::u32_ {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        let res = T::get_rxfh_key_size(data);
        res
    }

    unsafe extern "C" fn get_rxfh_indir_size_callback(
        dev: *mut bindings::net_device,
    ) -> bindings::u32_ {
        // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
        let res = T::get_rxfh_indir_size(data);
        res
    }

    unsafe extern "C" fn get_rxfh_callback(
        dev: *mut bindings::net_device,
        indir: *mut bindings::u32_,
        key: *mut bindings::u8_,
        hfunc: *mut bindings::u8_,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::get_rxfh(data, indir, key, hfunc)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn set_rxfh_callback(
        dev: *mut bindings::net_device,
        indir: *const bindings::u32_,
        key: *const bindings::u8_,
        hfunc: bindings::u8_,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
                let res = T::set_rxfh(data, indir, key, hfunc)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn get_rxnfc_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_rxnfc,
        rule_locs: *mut bindings::u32_,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let info = unsafe{EthtoolRxnfc::from_ptr(ptr)};
            let res =  T::get_rxnfc(data, info)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn set_rxnfc_callback(
        dev: *mut bindings::net_device,
        ptr: *mut bindings::ethtool_rxnfc,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::ethtool_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe {
                T::DataEthtoolOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `ptr` remains valid for the duration of this
            // function call.
            let info = unsafe{EthtoolRxnfc::from_ptr(ptr)};
            let res =  T::set_rxnfc(data, info)?;
            Ok(res.try_into().unwrap())
        }
    }

    /// Builds an instance of `struct ethtool_ops`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `netdev_priv` will result in a value
    /// that must be valid.
    pub(crate) const unsafe fn ethtool_ops_build() -> &'static bindings::ethtool_ops {
        &Self::ETHTOOL_OPS
    }

    const NET_DEVICE_OPS: bindings::net_device_ops = bindings::net_device_ops {
        ndo_open: if <T>::HAS_NDO_OPEN {
            Some(Self::ndo_open_callbacks)
        } else {
            None
        },
        ndo_stop: if <T>::HAS_NDO_STOP {
            Some(Self::ndo_stop_callbacks)
        } else {
            None
        },
        ndo_start_xmit: if <T>::HAS_NDO_START_XMIT {
            Some(Self::ndo_start_xmit_callbacks)
        } else {
            None
        },
        ndo_validate_addr: if <T>::HAS_NDO_VALIDATE_ADDR {
            Some(Self::ndo_validate_addr_callbacks)
        } else {
            None
        },
        ndo_set_mac_address: if <T>::HAS_NDO_SET_MAC_ADDRESS {
            Some(Self::ndo_set_mac_address_callbacks)
        } else {
            None
        },
        ndo_set_rx_mode: if <T>::HAS_NDO_SET_RX_MODE {
            Some(Self::ndo_set_rx_mode_callbacks)
        } else {
            None
        },
        ndo_get_stats64: if <T>::HAS_NDO_GET_STATS64 {
            Some(Self::ndo_get_stats64_callbacks)
        } else {
            None
        },
        ndo_vlan_rx_add_vid: if <T>::HAS_NDO_VLAN_RX_ADD_VID {
            Some(Self::ndo_vlan_rx_add_vid_callbacks)
        } else {
            None
        },
        ndo_vlan_rx_kill_vid: if <T>::HAS_NDO_VLAN_RX_KILL_VID {
            Some(Self::ndo_vlan_rx_kill_vid_callbacks)
        } else {
            None
        },
        // todo
        ndo_bpf: None,
        // todo
        ndo_xdp_xmit: None,
        ndo_features_check: if <T>::HAS_NDO_FEATURES_CHECK {
            Some(Self::ndo_features_check_callbacks)
        } else {
            None
        },
        ndo_get_phys_port_name: if <T>::HAS_NDO_GET_PHYS_PORT_NAME {
            Some(Self::ndo_get_phys_port_name_callbacks)
        } else {
            None
        },
        ndo_set_features: if <T>::HAS_NDO_SET_FEATURES {
            Some(Self::ndo_set_features_callbacks)
        } else {
            None
        },
        ndo_tx_timeout: if <T>::HAS_NDO_TX_TIMEOUT {
            Some(Self::ndo_tx_timeout_callbacks)
        } else {
            None
        },
        ndo_init: None,
        ndo_uninit: None,
        ndo_select_queue: None,
        ndo_change_rx_flags: None,
        ndo_do_ioctl: None,
        ndo_eth_ioctl: None,
        ndo_siocbond: None,
        ndo_siocwandev: None,
        ndo_siocdevprivate: None,
        ndo_set_config: None,
        ndo_change_mtu: None,
        ndo_neigh_setup: None,
        ndo_has_offload_stats: None,
        ndo_get_offload_stats: None,
        ndo_get_stats: None,
        ndo_set_vf_mac: None,
        ndo_set_vf_vlan: None,
        ndo_set_vf_rate: None,
        ndo_set_vf_spoofchk: None,
        ndo_set_vf_trust: None,
        ndo_get_vf_config: None,
        ndo_set_vf_link_state: None,
        ndo_get_vf_stats: None,
        ndo_set_vf_port: None,
        ndo_get_vf_port: None,
        ndo_get_vf_guid: None,
        ndo_set_vf_guid: None,
        ndo_set_vf_rss_query_en: None,
        ndo_setup_tc: None,
        ndo_rx_flow_steer: None,
        ndo_add_slave: None,
        ndo_del_slave: None,
        ndo_get_xmit_slave: None,
        ndo_sk_get_lower_dev: None,
        ndo_fix_features: None,
        ndo_neigh_construct: None,
        ndo_neigh_destroy: None,
        ndo_fdb_add: None,
        ndo_fdb_del: None,
        ndo_fdb_del_bulk: None,
        ndo_fdb_dump: None,
        ndo_fdb_get: None,
        ndo_bridge_setlink: None,
        ndo_bridge_getlink: None,
        ndo_bridge_dellink: None,
        ndo_change_carrier: None,
        ndo_get_phys_port_id: None,
        ndo_get_port_parent_id: None,
        ndo_dfwd_add_station: None,
        ndo_dfwd_del_station: None,
        ndo_set_tx_maxrate: None,
        ndo_get_iflink: None,
        ndo_fill_metadata_dst: None,
        ndo_set_rx_headroom: None,
        ndo_xdp_get_xmit_slave: None,
        ndo_xsk_wakeup: None,
        ndo_get_devlink_port: None,
        ndo_tunnel_ctl: None,
        ndo_get_peer_dev: None,
        ndo_fill_forward_path: None,
        ndo_get_tstamp: None,
    };

    unsafe extern "C" fn ndo_open_callbacks(dev: *mut bindings::net_device) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_open(data)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_stop_callbacks(dev: *mut bindings::net_device) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_stop(data)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_start_xmit_callbacks(
        skb: *mut bindings::sk_buff,
        dev: *mut bindings::net_device,
    ) -> bindings::netdev_tx_t {
        // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `skb` remains valid for the duration of this
        // function call.
        let skb = unsafe { SkBuff::from_ptr(skb) };
        let res = T::ndo_start_xmit(skb, data);
        res
    }

    unsafe extern "C" fn ndo_validate_addr_callbacks(
        dev: *mut bindings::net_device,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_validate_addr(data)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_set_mac_address_callbacks(
        dev: *mut bindings::net_device,
        addr: *mut core::ffi::c_void,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            // SAFETY: The C contract guarantees that `addr` remains valid for the duration of this
            // function call.
            let addr = unsafe{SocketAddr::V4(SocketAddrV4::from_ptr(addr))};
            let res = T::ndo_set_mac_address(data, addr)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_set_rx_mode_callbacks(dev: *mut bindings::net_device) {
        // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
        T::ndo_set_rx_mode(data);
    }

    unsafe extern "C" fn ndo_get_stats64_callbacks(
        dev: *mut bindings::net_device,
        storage: *mut bindings::rtnl_link_stats64,
    ) {
        // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `storage` remains valid for the duration of this
        // function call.
        let stat = unsafe { RtnlLinkStats64::from_ptr(storage) };
        T::ndo_get_stats64(data, stat);
    }

    unsafe extern "C" fn ndo_vlan_rx_add_vid_callbacks(
        dev: *mut bindings::net_device,
        proto: bindings::__be16,
        vid: bindings::u16_,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_vlan_rx_add_vid(data, proto, vid)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_vlan_rx_kill_vid_callbacks(
        dev: *mut bindings::net_device,
        proto: bindings::__be16,
        vid: bindings::u16_,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_vlan_rx_kill_vid(data, proto, vid)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_features_check_callbacks(
        skb_ptr: *mut bindings::sk_buff,
        dev: *mut bindings::net_device,
        features: bindings::netdev_features_t,
    ) -> bindings::netdev_features_t {
        // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
        // SAFETY: The C contract guarantees that `skb_ptr` remains valid for the duration of this
        // function call.
        let skb = unsafe { SkBuff::from_ptr(skb_ptr) };
        let res = T::ndo_features_check(data, skb, features);
        res
    }

    unsafe extern "C" fn ndo_get_phys_port_name_callbacks(
        dev: *mut bindings::net_device,
        name: *mut core::ffi::c_char,
        len: usize,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
             // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let buffer = unsafe { slice::from_raw_parts_mut(name as *mut u8, len) };
            let res = T::ndo_get_phys_port_name(data, buffer)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_set_features_callbacks(
        dev: *mut bindings::net_device,
        features: bindings::netdev_features_t,
    ) -> core::ffi::c_int {
        from_kernel_result! {
            // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
            // be valid.
            let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
            let res = T::ndo_set_features(data, features)?;
            Ok(res.try_into().unwrap())
        }
    }

    unsafe extern "C" fn ndo_tx_timeout_callbacks(
        dev: *mut bindings::net_device,
        txqueue: core::ffi::c_uint,
    ) {
        // By the safety requirements of `NetDeviceTables::net_device_ops_build`, we know that `data` must
        // be valid.
        let data = unsafe { T::DataNetdevOps::borrow(bindings::netdev_priv(dev)) };
        T::ndo_tx_timeout(data, txqueue);
    }

    /// Builds an instance of `struct net_device_ops`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `netdev_priv` will result in a value
    /// that must be valid.
    pub(crate) const unsafe fn net_device_ops_build() -> &'static bindings::net_device_ops {
        &Self::NET_DEVICE_OPS
    }
}
