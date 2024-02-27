// SPDX-License-Identifier: GPL-2.0

//! UDP socket wrapper.
//!
//! This module contains wrappers for a UDP Socket ([`UdpSocket`]).
//! The wrapper is just convenience structs around the generic [`Socket`] type.
//!
//! The API is inspired by the Rust standard library's [`UdpSocket`](https://doc.rust-lang.org/std/net/struct.UdpSocket.html).

use crate::error::Result;
use crate::net::addr::SocketAddr;
use crate::net::ip::IpProtocol;
use crate::net::socket::flags::{FlagSet, ReceiveFlag, SendFlag};
use crate::net::socket::{opts::SocketOption, MessageHeader, SockType, Socket};
use crate::net::AddressFamily;
use kernel::net::socket::opts::WritableOption;

/// A UDP socket.
///
/// Provides an interface to send and receive UDP packets, removing
/// all the socket functionality that is not needed for UDP.
///
/// # Examples
/// ```rust
/// use kernel::flag_set;
/// use kernel::net::udp::UdpSocket;
/// use kernel::net::addr::*;
///
/// let socket = UdpSocket::new().unwrap();
/// socket.bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000))).unwrap();
/// let mut buf = [0u8; 1024];
/// while let Ok((len, addr)) = socket.receive_from(&mut buf, flag_set!()) {
///     socket.send_to(&buf[..len], &addr, flag_set!()).unwrap();
/// }
/// ```
pub struct UdpSocket(pub(crate) Socket);

impl UdpSocket {
    /// Creates a UDP socket.
    ///
    /// Returns a [`UdpSocket`] on success.
    pub fn new() -> Result<Self> {
        Ok(Self(Socket::new(
            AddressFamily::Inet,
            SockType::Datagram,
            IpProtocol::Udp,
        )?))
    }

    /// Binds the socket to the given address.
    pub fn bind(&self, address: SocketAddr) -> Result {
        self.0.bind(address)
    }

    /// Returns the socket's local address.
    ///
    /// This function assumes the socket is bound,
    /// i.e. it must be called after [`bind()`](UdpSocket::bind).
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::udp::UdpSocket;
    /// use kernel::net::addr::*;
    ///
    /// let socket = UdpSocket::new().unwrap();
    /// let local_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000));
    /// socket.bind(local_addr).unwrap();
    /// assert_eq!(socket.sockname().unwrap(), local_addr);
    pub fn sockname(&self) -> Result<SocketAddr> {
        self.0.sockname()
    }

    /// Returns the socket's peer address.
    ///
    /// This function assumes the socket is connected,
    /// i.e. it must be called after [`connect()`](UdpSocket::connect).
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::udp::UdpSocket;
    /// use kernel::net::addr::*;
    ///
    /// let socket = UdpSocket::new().unwrap();
    /// let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000));
    /// socket.connect(&peer_addr).unwrap();
    /// assert_eq!(socket.peername().unwrap(), peer_addr);
    pub fn peername(&self) -> Result<SocketAddr> {
        self.0.peername()
    }

    /// Receive a message from the socket.
    ///
    /// The given flags are used to modify the behavior of the receive operation.
    /// See [`ReceiveFlag`] for more.
    ///
    /// The returned [`MessageHeader`] contains metadata about the received message.
    ///
    /// See [`Socket::receive_msg()`] for more.
    pub fn receive_msg(
        &self,
        buf: &mut [u8],
        flags: FlagSet<ReceiveFlag>,
    ) -> Result<(usize, MessageHeader)> {
        self.0.receive_msg(buf, flags)
    }

    /// Receives data from another socket.
    ///
    /// The given flags are used to modify the behavior of the receive operation.
    /// See [`ReceiveFlag`] for more.
    ///
    /// Returns the number of bytes received and the address of the sender.
    pub fn receive_from(
        &self,
        buf: &mut [u8],
        flags: FlagSet<ReceiveFlag>,
    ) -> Result<(usize, SocketAddr)> {
        self.0
            .receive_from(buf, flags)
            .map(|(size, addr)| (size, addr.unwrap()))
    }

    /// Sends data to another socket.
    ///
    /// The given flags are used to modify the behavior of the send operation.
    /// See [`SendFlag`] for more.
    ///
    /// Returns the number of bytes sent.
    pub fn send_to(
        &self,
        buf: &[u8],
        address: &SocketAddr,
        flags: FlagSet<SendFlag>,
    ) -> Result<usize> {
        self.0.send_to(buf, address, flags)
    }

    /// Connects the socket to the given address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::udp::UdpSocket;
    /// use kernel::net::addr::*;
    ///
    /// let socket = UdpSocket::new().unwrap();
    /// let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOOPBACK, 8000));
    /// socket.connect(&peer_addr).unwrap();
    /// ```
    pub fn connect(&self, address: &SocketAddr) -> Result {
        self.0.connect(address, 0)
    }

    /// Receives data from the connected socket.
    ///
    /// This function assumes the socket is connected,
    /// i.e. it must be called after [`connect()`](UdpSocket::connect).
    ///
    /// Returns the number of bytes received.
    pub fn receive(&self, buf: &mut [u8], flags: FlagSet<ReceiveFlag>) -> Result<usize> {
        self.0.receive(buf, flags)
    }

    /// Sends data to the connected socket.
    ///
    /// This function assumes the socket is connected,
    /// i.e. it must be called after [`connect()`](UdpSocket::connect).
    ///
    /// Returns the number of bytes sent.
    pub fn send(&self, buf: &[u8], flags: FlagSet<SendFlag>) -> Result<usize> {
        self.0.send(buf, flags)
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
