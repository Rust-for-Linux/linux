// SPDX-License-Identifier: GPL-2.0

//! TCP socket wrapper.
//!
//! This module contains wrappers for a TCP Socket ([`TcpListener`]) and an active
//! TCP connection ([`TcpStream`]).
//! The wrappers are just convenience structs around the generic [`Socket`] type.
//!
//! The API is inspired by the Rust standard library's [`TcpListener`](https://doc.rust-lang.org/std/net/struct.TcpListener.html) and [`TcpStream`](https://doc.rust-lang.org/std/net/struct.TcpStream.html).

use crate::error::Result;
use crate::net::addr::SocketAddr;
use crate::net::ip::IpProtocol;
use crate::net::socket::flags::{FlagSet, ReceiveFlag, SendFlag};
use crate::net::socket::opts::{SocketOption, WritableOption};
use crate::net::socket::{ShutdownCmd, SockType, Socket};
use crate::net::AddressFamily;
use kernel::net::socket::MessageHeader;

/// A TCP listener.
///
/// Wraps the [`Socket`] type to create a TCP-specific interface.
///
/// The wrapper abstracts away the generic Socket methods that a connection-oriented
/// protocol like TCP does not need.
///
/// # Examples
/// ```rust
/// use kernel::net::tcp::TcpListener;
/// use kernel::net::addr::*;
///
/// let listener = TcpListener::new(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
/// while let Ok(stream) = listener.accept() {
///   // ...
/// }
pub struct TcpListener(pub(crate) Socket);

impl TcpListener {
    /// Create a new TCP listener bound to the given address.
    ///
    /// The listener will be ready to accept connections.
    pub fn new(address: SocketAddr) -> Result<Self> {
        let socket = Socket::new(AddressFamily::Inet, SockType::Stream, IpProtocol::Tcp)?;
        socket.bind(address)?;
        socket.listen(128)?;
        Ok(Self(socket))
    }

    /// Returns the local address that this listener is bound to.
    ///
    /// See [`Socket::sockname()`] for more.
    pub fn sockname(&self) -> Result<SocketAddr> {
        self.0.sockname()
    }

    /// Returns an iterator over incoming connections.
    ///
    /// Each iteration will return a [`Result`] containing a [`TcpStream`] on success.
    /// See [`TcpIncoming`] for more.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::tcp::TcpListener;
    /// use kernel::net::addr::*;
    ///
    /// let listener = TcpListener::new(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
    /// for stream in listener.incoming() {
    ///    // ...
    /// }
    /// ```
    pub fn incoming(&self) -> TcpIncoming<'_> {
        TcpIncoming { listener: self }
    }

    /// Accepts an incoming connection.
    ///
    /// Returns a [`TcpStream`] on success.
    pub fn accept(&self) -> Result<TcpStream> {
        Ok(TcpStream(self.0.accept(true)?))
    }

    /// Sets the value of the given option.
    ///
    /// See [`Socket::set_option()`](Socket::set_option) for more.
    pub fn set_option<O>(&self, value: impl Into<O::Type>) -> Result
    where
        O: SocketOption + WritableOption,
    {
        self.0.set_option::<O>(value)
    }
}

/// An iterator over incoming connections from a [`TcpListener`].
///
/// Each iteration will return a [`Result`] containing a [`TcpStream`] on success.
/// The iterator will never return [`None`].
///
/// This struct is created by the [`TcpListener::incoming()`] method.
pub struct TcpIncoming<'a> {
    listener: &'a TcpListener,
}

impl Iterator for TcpIncoming<'_> {
    /// The item type of the iterator.
    type Item = Result<TcpStream>;

    /// Get the next connection from the listener.
    fn next(&mut self) -> Option<Self::Item> {
        Some(self.listener.accept())
    }
}

/// A TCP stream.
///
/// Represents an active TCP connection between two sockets.
/// The stream can be opened by the listener, with [`TcpListener::accept()`], or by
/// connecting to a remote address with [`TcpStream::connect()`].
/// The stream can be used to send and receive data.
///
/// See [`TcpListener`] for an example of how to create a [`TcpStream`].
pub struct TcpStream(pub(crate) Socket);

impl TcpStream {
    /// Opens a TCP stream by connecting to the given address.
    ///
    /// Returns a [`TcpStream`] on success.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::tcp::TcpStream;
    /// use kernel::net::addr::*;
    ///
    /// let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000));
    /// let stream = TcpStream::connect(&peer_addr).unwrap();
    /// ```
    pub fn connect(address: &SocketAddr) -> Result<Self> {
        let socket = Socket::new(AddressFamily::Inet, SockType::Stream, IpProtocol::Tcp)?;
        socket.connect(address, 0)?;
        Ok(Self(socket))
    }

    /// Returns the address of the remote peer of this connection.
    ///
    /// See [`Socket::peername()`] for more.
    pub fn peername(&self) -> Result<SocketAddr> {
        self.0.peername()
    }

    /// Returns the address of the local socket of this connection.
    ///
    /// See [`Socket::sockname()`] for more.
    pub fn sockname(&self) -> Result<SocketAddr> {
        self.0.sockname()
    }

    /// Receive data from the stream.
    /// The given flags are used to modify the behavior of the receive operation.
    /// See [`ReceiveFlag`] for more.
    ///
    /// Returns the number of bytes received.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::flag_set;
    /// use kernel::net::tcp::TcpListener;
    /// use kernel::net::addr::*;
    ///
    /// let listener = TcpListener::new(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
    /// while let Ok(stream) = listener.accept() {
    ///     let mut buf = [0u8; 1024];
    ///     while let Ok(len) = stream.receive(&mut buf, flag_set!()) {
    ///         // ...
    ///     }
    /// }
    /// ```
    pub fn receive(&self, buf: &mut [u8], flags: FlagSet<ReceiveFlag>) -> Result<usize> {
        self.0.receive(buf, flags)
    }

    /// Receive data from the stream and return the message header.
    ///
    /// The given flags are used to modify the behavior of the receive operation.
    ///
    /// Returns the number of bytes received and the message header, which contains
    /// information about the sender and the message.
    ///
    /// See [`Socket::receive_msg()`] for more.
    pub fn receive_msg(
        &self,
        buf: &mut [u8],
        flags: FlagSet<ReceiveFlag>,
    ) -> Result<(usize, MessageHeader)> {
        self.0.receive_msg(buf, flags)
    }

    /// Send data to the stream.
    ///
    /// The given flags are used to modify the behavior of the send operation.
    /// See [`SendFlag`] for more.
    ///
    /// Returns the number of bytes sent.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::flag_set;
    /// use kernel::net::tcp::TcpListener;
    /// use kernel::net::addr::*;
    ///
    /// let listener = TcpListener::new(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
    /// while let Ok(stream) = listener.accept() {
    ///     let mut buf = [0u8; 1024];
    ///     while let Ok(len) = stream.receive(&mut buf, flag_set!()) {
    ///         stream.send(&buf[..len], flag_set!())?;
    ///     }
    /// }
    /// ```
    pub fn send(&self, buf: &[u8], flags: FlagSet<SendFlag>) -> Result<usize> {
        self.0.send(buf, flags)
    }

    /// Manually shutdown some portion of the stream.
    /// See [`ShutdownCmd`] for more.
    ///
    /// This method is not required to be called, as the stream will be shutdown
    /// automatically when it is dropped.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::tcp::TcpListener;
    /// use kernel::net::addr::*;
    /// use kernel::net::socket::ShutdownCmd;
    ///
    /// let listener = TcpListener::new(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
    /// while let Ok(stream) = listener.accept() {
    ///    // ...
    ///    stream.shutdown(ShutdownCmd::Both)?;
    /// }
    /// ```
    pub fn shutdown(&self, how: ShutdownCmd) -> Result {
        self.0.shutdown(how)
    }
}

impl Drop for TcpStream {
    /// Shutdown the stream.
    ///
    /// This method ignores the outcome of the shutdown operation: whether the stream
    /// is successfully shutdown or not, the stream will be dropped anyways.
    fn drop(&mut self) {
        self.0.shutdown(ShutdownCmd::Both).ok();
    }
}
