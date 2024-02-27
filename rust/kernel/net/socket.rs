// SPDX-License-Identifier: GPL-2.0

//! Socket API.
//!
//! This module contains the Socket layer kernel APIs that have been wrapped for usage by Rust code
//! in the kernel.
//!
//! C header: [`include/linux/socket.h`](../../../../include/linux/socket.h)
//!
//! This API is inspired by the Rust std::net Socket API, but is not a direct port.
//! The main difference is that the Rust std::net API is designed for user-space, while this API
//! is designed for kernel-space.
//! Rust net API: <https://doc.rust-lang.org/std/net/index.html>

use super::*;
use crate::error::{to_result, Result};
use crate::net::addr::*;
use crate::net::ip::IpProtocol;
use crate::net::socket::opts::{OptionsLevel, WritableOption};
use core::cmp::max;
use core::marker::PhantomData;
use flags::*;
use kernel::net::socket::opts::SocketOption;

pub mod flags;
pub mod opts;

/// The socket type.
pub enum SockType {
    /// Stream socket (e.g. TCP)
    Stream = bindings::sock_type_SOCK_STREAM as isize,
    /// Connectionless socket (e.g. UDP)
    Datagram = bindings::sock_type_SOCK_DGRAM as isize,
    /// Raw socket
    Raw = bindings::sock_type_SOCK_RAW as isize,
    /// Reliably-delivered message
    Rdm = bindings::sock_type_SOCK_RDM as isize,
    /// Sequenced packet stream
    Seqpacket = bindings::sock_type_SOCK_SEQPACKET as isize,
    /// Datagram Congestion Control Protocol socket
    Dccp = bindings::sock_type_SOCK_DCCP as isize,
    /// Packet socket
    Packet = bindings::sock_type_SOCK_PACKET as isize,
}

/// The socket shutdown command.
pub enum ShutdownCmd {
    /// Disallow further receive operations.
    Read = bindings::sock_shutdown_cmd_SHUT_RD as isize,
    /// Disallow further send operations.
    Write = bindings::sock_shutdown_cmd_SHUT_WR as isize,
    /// Disallow further send and receive operations.
    Both = bindings::sock_shutdown_cmd_SHUT_RDWR as isize,
}

/// A generic socket.
///
/// Wraps a `struct socket` from the kernel.
/// See [include/linux/socket.h](../../../../include/linux/socket.h) for more information.
///
/// The wrapper offers high-level methods for common operations on the socket.
/// More fine-grained control is possible by using the C bindings directly.
///
/// # Example
/// A simple TCP echo server:
/// ```rust
/// use kernel::flag_set;
/// use kernel::net::addr::{Ipv4Addr, SocketAddr, SocketAddrV4};
/// use kernel::net::{AddressFamily, init_net};
/// use kernel::net::ip::IpProtocol;
/// use kernel::net::socket::{Socket, SockType};
///
/// let socket = Socket::new_kern(
///     init_net(),
///     AddressFamily::Inet,
///     SockType::Stream,
///     IpProtocol::Tcp,
/// )?;
/// socket.bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000)))?;
/// socket.listen(10)?;
/// while let Ok(peer) = socket.accept(true) {
///     let mut buf = [0u8; 1024];
///     peer.receive(&mut buf, flag_set!())?;
///     peer.send(&buf, flag_set!())?;
/// }
/// ```
/// A simple UDP echo server:
/// ```rust
/// use kernel::net::addr::{Ipv4Addr, SocketAddr, SocketAddrV4};
/// use kernel::net::{AddressFamily, init_net};
/// use kernel::net::ip::IpProtocol;
/// use kernel::net::socket::{Socket, SockType};
/// use kernel::flag_set;
///
/// let socket = Socket::new_kern(init_net(), AddressFamily::Inet, SockType::Datagram, IpProtocol::Udp)?;///
/// socket.bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000)))?;
/// let mut buf = [0u8; 1024];
/// while let Ok((len, sender_opt)) = socket.receive_from(&mut buf, flag_set!()) {
///     let sender: SocketAddr = sender_opt.expect("Sender address is always available for UDP");
///     socket.send_to(&buf[..len], &sender, flag_set!())?;
/// }
/// ```
///
/// # Invariants
///
/// The socket pointer is valid for the lifetime of the wrapper.
#[repr(transparent)]
pub struct Socket(*mut bindings::socket);

/// Getters and setters of socket internal fields.
///
/// Not all fields are currently supported: hopefully, this will be improved in the future.
impl Socket {
    /// Retrieve the flags associated with the socket.
    ///
    /// Unfortunately, these flags cannot be represented as a [`FlagSet`], since [`SocketFlag`]s
    /// are not represented as masks but as the index of the bit they represent.
    ///
    /// An enum could be created, containing masks instead of indexes, but this could create
    /// confusion with the C side.
    ///
    /// The methods [`Socket::has_flag`] and [`Socket::set_flags`] can be used to check and set individual flags.
    pub fn flags(&self) -> u64 {
        unsafe { (*self.0).flags }
    }

    /// Set the flags associated with the socket.
    pub fn set_flags(&self, flags: u64) {
        unsafe {
            (*self.0).flags = flags;
        }
    }

    /// Checks if the socket has a specific flag.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::{Socket, flags::SocketFlag, SockType};
    /// use kernel::net::AddressFamily;
    /// use kernel::net::ip::IpProtocol;
    ///
    /// let socket = Socket::new(AddressFamily::Inet, SockType::Datagram, IpProtocol::Udp)?;
    /// assert_eq!(socket.has_flag(SocketFlag::CustomSockOpt), false);
    /// ```
    pub fn has_flag(&self, flag: SocketFlag) -> bool {
        bindings::__BindgenBitfieldUnit::<[u8; 8], u8>::new(self.flags().to_be_bytes())
            .get_bit(flag as _)
    }

    /// Sets a flag on the socket.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::{Socket, flags::SocketFlag, SockType};
    /// use kernel::net::AddressFamily;
    /// use kernel::net::ip::IpProtocol;
    ///
    /// let socket = Socket::new(AddressFamily::Inet, SockType::Datagram, IpProtocol::Udp)?;
    /// assert_eq!(socket.has_flag(SocketFlag::CustomSockOpt), false);
    /// socket.set_flag(SocketFlag::CustomSockOpt, true);
    /// assert_eq!(socket.has_flag(SocketFlag::CustomSockOpt), true);
    /// ```
    pub fn set_flag(&self, flag: SocketFlag, value: bool) {
        let flags_width = core::mem::size_of_val(&self.flags()) * 8;
        let mut flags =
            bindings::__BindgenBitfieldUnit::<[u8; 8], u8>::new(self.flags().to_be_bytes());
        flags.set_bit(flag as _, value);
        self.set_flags(flags.get(0, flags_width as _));
    }

    /// Consumes the socket and returns the underlying pointer.
    ///
    /// The pointer is valid for the lifetime of the wrapper.
    ///
    /// # Safety
    /// The caller must ensure that the pointer is not used after the wrapper is dropped.
    pub unsafe fn into_inner(self) -> *mut bindings::socket {
        self.0
    }

    /// Returns the underlying pointer.
    ///
    /// The pointer is valid for the lifetime of the wrapper.
    ///
    /// # Safety
    /// The caller must ensure that the pointer is not used after the wrapper is dropped.
    pub unsafe fn as_inner(&self) -> *mut bindings::socket {
        self.0
    }
}

/// Socket API implementation
impl Socket {
    /// Private utility function to create a new socket by calling a function.
    /// The function is generic over the creation function.
    ///
    /// # Arguments
    /// * `create_fn`: A function that initiates the socket given as parameter.
    ///                The function must return 0 on success and a negative error code on failure.
    fn base_new<T>(create_fn: T) -> Result<Self>
    where
        T: (FnOnce(*mut *mut bindings::socket) -> core::ffi::c_int),
    {
        let mut socket_ptr: *mut bindings::socket = core::ptr::null_mut();
        to_result(create_fn(&mut socket_ptr))?;
        Ok(Self(socket_ptr))
    }

    /// Create a new socket.
    ///
    /// Wraps the `sock_create` function.
    pub fn new(family: AddressFamily, type_: SockType, proto: IpProtocol) -> Result<Self> {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        Self::base_new(|socket_ptr| unsafe {
            bindings::sock_create(family as _, type_ as _, proto as _, socket_ptr)
        })
    }

    /// Create a new socket in a specific namespace.
    ///
    /// Wraps the `sock_create_kern` function.
    pub fn new_kern(
        ns: &Namespace,
        family: AddressFamily,
        type_: SockType,
        proto: IpProtocol,
    ) -> Result<Self> {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        Self::base_new(|socket_ptr| unsafe {
            bindings::sock_create_kern(ns.0.get(), family as _, type_ as _, proto as _, socket_ptr)
        })
    }

    /// Creates a new "lite" socket.
    ///
    /// Wraps the `sock_create_lite` function.
    ///
    /// This is a lighter version of `sock_create` that does not perform any sanity check.
    pub fn new_lite(family: AddressFamily, type_: SockType, proto: IpProtocol) -> Result<Self> {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        Self::base_new(|socket_ptr| unsafe {
            bindings::sock_create_lite(family as _, type_ as _, proto as _, socket_ptr)
        })
    }

    /// Binds the socket to a specific address.
    ///
    /// Wraps the `kernel_bind` function.
    pub fn bind(&self, address: SocketAddr) -> Result {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        to_result(unsafe {
            bindings::kernel_bind(self.0, address.as_ptr() as _, address.size() as i32)
        })
    }

    /// Connects the socket to a specific address.
    ///
    /// Wraps the `kernel_connect` function.
    ///
    /// The socket must be a connection-oriented socket.
    /// If the socket is not bound, it will be bound to a random local address.
    ///
    /// # Example
    /// ```rust
    /// use kernel::net::{AddressFamily, init_net};
    /// use kernel::net::addr::{Ipv4Addr, SocketAddr, SocketAddrV4};
    /// use kernel::net::ip::IpProtocol;
    /// use kernel::net::socket::{Socket, SockType};
    ///
    /// let socket = Socket::new_kern(init_net(), AddressFamily::Inet, SockType::Stream, IpProtocol::Tcp)?;
    /// socket.bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000)))?;
    /// socket.listen(10)?;
    pub fn listen(&self, backlog: i32) -> Result {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        to_result(unsafe { bindings::kernel_listen(self.0, backlog) })
    }

    /// Accepts a connection on a socket.
    ///
    /// Wraps the `kernel_accept` function.
    pub fn accept(&self, block: bool) -> Result<Socket> {
        let mut new_sock = core::ptr::null_mut();
        let flags: i32 = if block { 0 } else { bindings::O_NONBLOCK as _ };

        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        to_result(unsafe { bindings::kernel_accept(self.0, &mut new_sock, flags as _) })?;

        Ok(Self(new_sock))
    }

    /// Returns the address the socket is bound to.
    ///
    /// Wraps the `kernel_getsockname` function.
    pub fn sockname(&self) -> Result<SocketAddr> {
        let mut addr: SocketAddrStorage = SocketAddrStorage::default();

        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        unsafe {
            to_result(bindings::kernel_getsockname(
                self.0,
                &mut addr as *mut _ as _,
            ))
        }
        .and_then(|_| SocketAddr::try_from_raw(addr))
    }

    /// Returns the address the socket is connected to.
    ///
    /// Wraps the `kernel_getpeername` function.
    ///
    /// The socket must be connected.
    pub fn peername(&self) -> Result<SocketAddr> {
        let mut addr: SocketAddrStorage = SocketAddrStorage::default();

        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        unsafe {
            to_result(bindings::kernel_getpeername(
                self.0,
                &mut addr as *mut _ as _,
            ))
        }
        .and_then(|_| SocketAddr::try_from_raw(addr))
    }

    /// Connects the socket to a specific address.
    ///
    /// Wraps the `kernel_connect` function.
    pub fn connect(&self, address: &SocketAddr, flags: i32) -> Result {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        unsafe {
            to_result(bindings::kernel_connect(
                self.0,
                address.as_ptr() as _,
                address.size() as _,
                flags,
            ))
        }
    }

    /// Shuts down the socket.
    ///
    /// Wraps the `kernel_sock_shutdown` function.
    pub fn shutdown(&self, how: ShutdownCmd) -> Result {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        unsafe { to_result(bindings::kernel_sock_shutdown(self.0, how as _)) }
    }

    /// Receive a message from the socket.
    ///
    /// This function is the lowest-level receive function. It is used by the other receive functions.
    ///
    /// The `flags` parameter is a set of flags that control the behavior of the function.
    /// The flags are described in the [`ReceiveFlag`] enum.
    ///
    /// The returned Message is a wrapper for `msghdr` and it contains the header information about the message,
    /// including the sender address (if present) and the flags.
    ///
    /// The data message is written to the provided buffer and the number of bytes written is returned together with the header.
    ///
    /// Wraps the `kernel_recvmsg` function.
    pub fn receive_msg(
        &self,
        bytes: &mut [u8],
        flags: FlagSet<ReceiveFlag>,
    ) -> Result<(usize, MessageHeader)> {
        let addr = SocketAddrStorage::default();

        let mut msg = bindings::msghdr {
            msg_name: &addr as *const _ as _,
            ..Default::default()
        };

        let mut vec = bindings::kvec {
            iov_base: bytes.as_mut_ptr() as _,
            iov_len: bytes.len() as _,
        };

        // SAFETY: FFI call; the socket address is valid for the lifetime of the wrapper.
        let size = unsafe {
            bindings::kernel_recvmsg(
                self.0,
                &mut msg as _,
                &mut vec,
                1,
                bytes.len() as _,
                flags.value() as _,
            )
        };
        to_result(size)?;

        let addr: Option<SocketAddr> = SocketAddr::try_from_raw(addr).ok();

        Ok((size as _, MessageHeader::new(msg, addr)))
    }

    /// Receives data from a remote socket and returns the bytes read and the sender address.
    ///
    /// Used by connectionless sockets to retrieve the sender of the message.
    /// If the socket is connection-oriented, the sender address will be `None`.
    ///
    /// The function abstracts the usage of the `struct msghdr` type.
    /// See [Socket::receive_msg] for more information.
    pub fn receive_from(
        &self,
        bytes: &mut [u8],
        flags: FlagSet<ReceiveFlag>,
    ) -> Result<(usize, Option<SocketAddr>)> {
        self.receive_msg(bytes, flags)
            .map(|(size, hdr)| (size, hdr.into()))
    }

    /// Receives data from a remote socket and returns only the bytes read.
    ///
    /// Used by connection-oriented sockets, where the sender address is the connected peer.
    pub fn receive(&self, bytes: &mut [u8], flags: FlagSet<ReceiveFlag>) -> Result<usize> {
        let (size, _) = self.receive_from(bytes, flags)?;
        Ok(size)
    }

    /// Sends a message to a remote socket.
    ///
    /// Wraps the `kernel_sendmsg` function.
    ///
    /// Crate-public to allow its usage only in the kernel crate.
    /// In the future, this function could be made public, accepting a [`Message`] as input,
    /// but with the current API, it does not give any advantage.
    pub(crate) fn send_msg(
        &self,
        bytes: &[u8],
        mut message: bindings::msghdr,
        flags: FlagSet<SendFlag>,
    ) -> Result<usize> {
        let mut vec = bindings::kvec {
            iov_base: bytes.as_ptr() as _,
            iov_len: bytes.len() as _,
        };
        message.msg_flags = flags.value() as _;

        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        let size = unsafe {
            bindings::kernel_sendmsg(
                self.0,
                &message as *const _ as _,
                &mut vec,
                1,
                bytes.len() as _,
            )
        };
        to_result(size)?;
        Ok(size as _)
    }

    /// Sends a message to a remote socket and returns the bytes sent.
    ///
    /// The `flags` parameter is a set of flags that control the behavior of the function.
    /// The flags are described in the [`SendFlag`] enum.
    pub fn send(&self, bytes: &[u8], flags: FlagSet<SendFlag>) -> Result<usize> {
        self.send_msg(bytes, bindings::msghdr::default(), flags)
    }

    /// Sends a message to a specific remote socket address and returns the bytes sent.
    ///
    /// The `flags` parameter is a set of flags that control the behavior of the function.
    /// The flags are described in the [`SendFlag`] enum.
    pub fn send_to(
        &self,
        bytes: &[u8],
        address: &SocketAddr,
        flags: FlagSet<SendFlag>,
    ) -> Result<usize> {
        let message = bindings::msghdr {
            msg_name: address.as_ptr() as _,
            msg_namelen: address.size() as _,
            ..Default::default()
        };
        self.send_msg(bytes, message, flags)
    }

    /// Sets an option on the socket.
    ///
    /// Wraps the `sock_setsockopt` function.
    ///
    /// The generic type `T` is the type of the option value.
    /// See the [options module](opts) for the type and extra information about each option.
    ///
    /// Unfortunately, options can only be set but not retrieved.
    /// This is because the kernel functions to retrieve options are not exported by the kernel.
    /// The only exported functions accept user-space pointers, and are therefore not usable in the kernel.
    ///
    /// # Example
    /// ```
    /// use kernel::net::AddressFamily;
    /// use kernel::net::ip::IpProtocol;use kernel::net::socket::{Socket, SockType};
    /// use kernel::net::socket::opts;
    ///
    /// let socket = Socket::new(AddressFamily::Inet, SockType::Datagram, IpProtocol::Udp)?;
    /// socket.set_option::<opts::ip::BindAddressNoPort>(true)?;
    /// ```
    pub fn set_option<O>(&self, value: impl Into<O::Type>) -> Result
    where
        O: SocketOption + WritableOption,
    {
        let value_ptr = SockPtr::new(&value);

        // The minimum size is the size of an integer.
        let min_size = core::mem::size_of::<core::ffi::c_int>();
        let size = max(core::mem::size_of::<O::Type>(), min_size);

        if O::level() == OptionsLevel::Socket && !self.has_flag(SocketFlag::CustomSockOpt) {
            // SAFETY: FFI call;
            // the address is valid for the lifetime of the wrapper;
            // the size is at least the size of an integer;
            // the level and name of the option are valid and coherent.
            to_result(unsafe {
                bindings::sock_setsockopt(
                    self.0,
                    O::level() as isize as _,
                    O::value() as _,
                    value_ptr.to_raw() as _,
                    size as _,
                )
            })
        } else {
            // SAFETY: FFI call;
            // the address is valid for the lifetime of the wrapper;
            // the size is at least the size of an integer;
            // the level and name of the option are valid and coherent.
            to_result(unsafe {
                (*(*self.0).ops)
                    .setsockopt
                    .map(|f| {
                        f(
                            self.0,
                            O::level() as _,
                            O::value() as _,
                            value_ptr.to_raw() as _,
                            size as _,
                        )
                    })
                    .unwrap_or(-(bindings::EOPNOTSUPP as i32))
            })
        }
    }
}

impl Drop for Socket {
    /// Closes and releases the socket.
    ///
    /// Wraps the `sock_release` function.
    fn drop(&mut self) {
        // SAFETY: FFI call; the address is valid for the lifetime of the wrapper.
        unsafe {
            bindings::sock_release(self.0);
        }
    }
}

// SAFETY: sockets are thread-safe; synchronization is handled by the kernel.
unsafe impl Send for Socket {}
unsafe impl Sync for Socket {}

/// Socket header message.
///
/// Wraps the `msghdr` structure.
/// This struct provides a safe interface to the `msghdr` structure.
///
/// The instances of this struct are only created by the `receive` methods of the [`Socket`] struct.
///
/// # Invariants
/// The `msg_name` in the wrapped `msghdr` object is always null; the address is stored in the `MessageHeader` object
/// and can be retrieved with the [`MessageHeader::address`] method.
#[derive(Clone, Copy)]
pub struct MessageHeader(pub(crate) bindings::msghdr, pub(crate) Option<SocketAddr>);

impl MessageHeader {
    /// Returns the address of the message.
    pub fn address(&self) -> Option<&SocketAddr> {
        self.1.as_ref()
    }

    /// Returns the flags of the message.
    pub fn flags(&self) -> FlagSet<MessageFlag> {
        FlagSet::from(self.0.msg_flags as isize)
    }

    /// Consumes the message header and returns the underlying `msghdr` structure.
    ///
    /// The returned msghdr will have a null pointer for the address.
    pub fn into_raw(self) -> bindings::msghdr {
        self.0
    }

    /// Creates a new message header.
    ///
    /// The `msg_name` of the field gets replaced with a NULL pointer.
    pub(crate) fn new(mut hdr: bindings::msghdr, addr: Option<SocketAddr>) -> Self {
        hdr.msg_name = core::ptr::null_mut();
        Self(hdr, addr)
    }
}

impl From<MessageHeader> for Option<SocketAddr> {
    /// Consumes the message header and returns the contained address.
    fn from(hdr: MessageHeader) -> Self {
        hdr.1
    }
}

impl From<MessageHeader> for bindings::msghdr {
    /// Consumes the message header and returns the underlying `msghdr` structure.
    ///
    /// The returned msghdr will have a null pointer for the address.
    ///
    /// This function is actually supposed to be crate-public, since bindings are not supposed to be
    /// used outside the kernel library.
    /// However, until the support for `msghdr` is not complete, specific needs might be satisfied
    /// only by using directly the underlying `msghdr` structure.
    fn from(hdr: MessageHeader) -> Self {
        hdr.0
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct SockPtr<'a>(bindings::sockptr_t, PhantomData<&'a ()>);

impl<'a> SockPtr<'a> {
    fn new<T>(value: &'a T) -> Self
    where
        T: Sized,
    {
        let mut sockptr = bindings::sockptr_t::default();
        sockptr.__bindgen_anon_1.kernel = value as *const T as _;
        sockptr._bitfield_1 = bindings::__BindgenBitfieldUnit::new([1; 1usize]); // kernel ptr
        SockPtr(sockptr, PhantomData)
    }

    fn to_raw(self) -> bindings::sockptr_t {
        self.0
    }
}
