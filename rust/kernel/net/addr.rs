// SPDX-License-Identifier: GPL-2.0

//! Network address types.
//!
//! This module contains the types and APIs related to network addresses.
//! The methods and types of this API are inspired by the [Rust standard library's `std::net` module](https://doc.rust-lang.org/std/net/index.html),
//! but have been ported to use the kernel's C APIs.

use crate::error::{code, Error, Result};
use crate::net::{init_net, AddressFamily, Namespace};
use crate::str::{CStr, CString};
use crate::{c_str, fmt};
use core::cmp::Ordering;
use core::fmt::{Debug, Display, Formatter};
use core::hash::{Hash, Hasher};
use core::mem::MaybeUninit;
use core::ptr;
use core::str::FromStr;

/// An IPv4 address.
///
/// Wraps a `struct in_addr`.
#[derive(Default, Copy, Clone)]
#[repr(transparent)]
pub struct Ipv4Addr(pub(crate) bindings::in_addr);

impl Ipv4Addr {
    /// The maximum length of an IPv4 address string.
    ///
    /// This is the length of the string representation of the address.
    /// It does not include the null terminator.
    pub const MAX_STRING_LEN: usize = 15;

    /// Create a new IPv4 address from four 8-bit integers.
    ///
    /// The IP address will be `a.b.c.d`.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::new(192, 168, 0, 1);
    /// ```
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self::from_bits(u32::from_be_bytes([a, b, c, d]))
    }

    /// Get the octets of the address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::new(192, 168, 0, 1);
    /// let expected = [192, 168, 0, 1];
    /// assert_eq!(addr.octets(), &expected);
    /// ```
    pub const fn octets(&self) -> &[u8; 4] {
        // SAFETY: The s_addr field is a 32-bit integer, which is the same size as the array.
        unsafe { &*(&self.0.s_addr as *const _ as *const [u8; 4]) }
    }

    /// Create a new IPv4 address from a 32-bit integer.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::from_bits(0xc0a80001);
    /// assert_eq!(addr, Ipv4Addr::new(192, 168, 0, 1));
    /// ```
    pub const fn from_bits(bits: u32) -> Self {
        Ipv4Addr(bindings::in_addr {
            s_addr: bits.to_be(),
        })
    }

    /// Get the 32-bit integer representation of the address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::new(192, 168, 0, 1);
    /// assert_eq!(addr.to_bits(), 0xc0a80001);
    /// ```
    pub const fn to_bits(&self) -> u32 {
        u32::from_be(self.0.s_addr)
    }

    /// The broadcast address: `255.255.255.255`
    ///
    /// Used to send a message to all hosts on the network.
    pub const BROADCAST: Self = Self::new(255, 255, 255, 255);

    /// "None" address
    ///
    /// Can be used as return value to indicate an error.
    pub const NONE: Self = Self::new(255, 255, 255, 255);

    /// The "any" address: `0.0.0.0`
    /// Used to accept any incoming message.
    pub const UNSPECIFIED: Self = Self::new(0, 0, 0, 0);

    /// A dummy address: `192.0.0.8`
    /// Used as ICMP reply source if no address is set.
    pub const DUMMY: Self = Self::new(192, 0, 0, 8);

    /// The loopback address: `127.0.0.1`
    /// Used to send a message to the local host.
    pub const LOOPBACK: Self = Self::new(127, 0, 0, 1);
}

impl From<[u8; 4]> for Ipv4Addr {
    /// Create a new IPv4 address from an array of 8-bit integers.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::from([192, 168, 0, 1]);
    /// assert_eq!(addr, Ipv4Addr::new(192, 168, 0, 1));
    /// ```
    fn from(octets: [u8; 4]) -> Self {
        Self::new(octets[0], octets[1], octets[2], octets[3])
    }
}

impl From<Ipv4Addr> for u32 {
    /// Get the 32-bit integer representation of the address.
    ///
    /// This is the same as calling [`Ipv4Addr::to_bits`].
    fn from(addr: Ipv4Addr) -> Self {
        addr.to_bits()
    }
}

impl From<u32> for Ipv4Addr {
    /// Create a new IPv4 address from a 32-bit integer.
    ///
    /// This is the same as calling [`Ipv4Addr::from_bits`].
    fn from(bits: u32) -> Self {
        Self::from_bits(bits)
    }
}

impl PartialEq<Ipv4Addr> for Ipv4Addr {
    /// Compare two IPv4 addresses.
    ///
    /// Returns `true` if the addresses are made up of the same bytes.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr1 = Ipv4Addr::new(192, 168, 0, 1);
    /// let addr2 = Ipv4Addr::new(192, 168, 0, 1);
    /// assert_eq!(addr1, addr2);
    ///
    /// let addr3 = Ipv4Addr::new(192, 168, 0, 2);
    /// assert_ne!(addr1, addr3);
    /// ```
    fn eq(&self, other: &Ipv4Addr) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Eq for Ipv4Addr {}

impl Hash for Ipv4Addr {
    /// Hash an IPv4 address.
    ///
    /// The trait cannot be derived because the `in_addr` struct does not implement `Hash`.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_bits().hash(state)
    }
}

impl PartialOrd for Ipv4Addr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_bits().partial_cmp(&other.to_bits())
    }
}

impl Ord for Ipv4Addr {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bits().cmp(&other.to_bits())
    }
}

/// An IPv6 address.
///
/// Wraps a `struct in6_addr`.
#[derive(Default, Copy, Clone)]
#[repr(transparent)]
pub struct Ipv6Addr(pub(crate) bindings::in6_addr);

impl Ipv6Addr {
    /// The maximum length of an IPv6 address string.
    ///
    /// This is the length of the string representation of the address.
    /// It does not include the null terminator.
    pub const MAX_STRING_LEN: usize = 45;

    /// Create a new IPv6 address from eight 16-bit integers.
    ///
    /// The 16-bit integers are transformed in network order.
    ///
    /// The IP address will be `a:b:c:d:e:f:g:h`.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334);
    /// ```
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

    /// Get the octets of the address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334);
    /// let expected = [0x20, 0x01, 0x0d, 0xb8, 0x85, 0xa3, 0x00, 0x00, 0x00, 0x00, 0x8a, 0x2e, 0x03, 0x70, 0x73, 0x34];
    /// assert_eq!(addr.octets(), &expected);
    /// ```
    pub const fn octets(&self) -> &[u8; 16] {
        // SAFETY: The u6_addr8 field is a [u8; 16] array.
        unsafe { &self.0.in6_u.u6_addr8 }
    }

    /// Get the segments of the address.
    ///
    /// A segment is a 16-bit integer.
    /// The segments are in network order.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334);
    /// let expected = [0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334];
    /// assert_eq!(addr.segments(), &expected);
    /// ```
    pub const fn segments(&self) -> &[u16; 8] {
        // SAFETY: The u6_addr16 field is a [u16; 8] array.
        unsafe { &self.0.in6_u.u6_addr16 }
    }

    /// Create a 128-bit integer representation of the address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334);
    /// assert_eq!(addr.to_bits(), 0x20010db885a3000000008a2e03707334);
    /// ```
    pub fn to_bits(&self) -> u128 {
        u128::from_be_bytes(*self.octets() as _)
    }

    /// Create a new IPv6 address from a 128-bit integer.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::from_bits(0x20010db885a3000000008a2e03707334);
    /// assert_eq!(addr, Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334));
    /// ```
    pub const fn from_bits(bits: u128) -> Self {
        Ipv6Addr(bindings::in6_addr {
            in6_u: bindings::in6_addr__bindgen_ty_1 {
                u6_addr8: bits.to_be_bytes() as _,
            },
        })
    }

    /// The "any" address: `::`
    ///
    /// Used to accept any incoming message.
    /// Should not be used as a destination address.
    pub const ANY: Self = Self::new(0, 0, 0, 0, 0, 0, 0, 0);

    /// The loopback address: `::1`
    ///
    /// Used to send a message to the local host.
    pub const LOOPBACK: Self = Self::new(0, 0, 0, 0, 0, 0, 0, 1);
}

impl From<[u16; 8]> for Ipv6Addr {
    fn from(value: [u16; 8]) -> Self {
        Self(bindings::in6_addr {
            in6_u: bindings::in6_addr__bindgen_ty_1 { u6_addr16: value },
        })
    }
}

impl From<[u8; 16]> for Ipv6Addr {
    fn from(value: [u8; 16]) -> Self {
        Self(bindings::in6_addr {
            in6_u: bindings::in6_addr__bindgen_ty_1 { u6_addr8: value },
        })
    }
}

impl From<Ipv6Addr> for u128 {
    fn from(addr: Ipv6Addr) -> Self {
        addr.to_bits()
    }
}

impl From<u128> for Ipv6Addr {
    fn from(bits: u128) -> Self {
        Self::from_bits(bits)
    }
}

impl PartialEq for Ipv6Addr {
    fn eq(&self, other: &Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Eq for Ipv6Addr {}

impl Hash for Ipv6Addr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_bits().hash(state)
    }
}

impl PartialOrd for Ipv6Addr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_bits().partial_cmp(&other.to_bits())
    }
}

impl Ord for Ipv6Addr {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bits().cmp(&other.to_bits())
    }
}

/// A wrapper for a generic socket address.
///
/// Wraps a C `struct sockaddr_storage`.
/// Unlike [`SocketAddr`], this struct is meant to be used internally only,
/// as a parameter for kernel function calls.
#[repr(transparent)]
#[derive(Copy, Clone, Default)]
pub(crate) struct SocketAddrStorage(pub(crate) bindings::__kernel_sockaddr_storage);

impl SocketAddrStorage {
    /// Returns the family of the address.
    pub(crate) fn family(&self) -> Result<AddressFamily, Error> {
        // SAFETY: The union access is safe because the `ss_family` field is always valid.
        let val: isize = unsafe { self.0.__bindgen_anon_1.__bindgen_anon_1.ss_family as _ };
        AddressFamily::try_from(val)
    }

    pub(crate) fn into<T: GenericSocketAddr>(self) -> T {
        // SAFETY: The `self.0` field is a `struct sockaddr_storage` which is guaranteed to be large enough to hold any socket address.
        unsafe { *(&self.0 as *const _ as *const T) }
    }
}

/// A generic Socket Address. Acts like a `struct sockaddr_storage`.
/// `sockaddr_storage` is used instead of `sockaddr` because it is guaranteed to be large enough to hold any socket address.
///
/// The purpose of this enum is to be used as a generic parameter for functions that can take any type of address.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SocketAddr {
    /// An IPv4 address.
    V4(SocketAddrV4),
    /// An IPv6 address.
    V6(SocketAddrV6),
}

impl SocketAddr {
    /// Returns the size in bytes of the concrete address contained.
    ///
    /// Used in the kernel functions that take a parameter with the size of the socket address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddr, SocketAddrV4};
    /// assert_eq!(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80)).size(),
    ///           core::mem::size_of::<SocketAddrV4>());
    pub fn size(&self) -> usize {
        match self {
            SocketAddr::V4(_) => SocketAddrV4::size(),
            SocketAddr::V6(_) => SocketAddrV6::size(),
        }
    }

    /// Returns the address family of the concrete address contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddr, SocketAddrV4};
    /// use kernel::net::AddressFamily;
    /// assert_eq!(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80)).family(),
    ///          AddressFamily::Inet);
    /// ```
    pub fn family(&self) -> AddressFamily {
        match self {
            SocketAddr::V4(_) => AddressFamily::Inet,
            SocketAddr::V6(_) => AddressFamily::Inet6,
        }
    }

    /// Returns a pointer to the C `struct sockaddr_storage` contained.
    /// Used in the kernel functions that take a pointer to a socket address.
    pub(crate) fn as_ptr(&self) -> *const SocketAddrStorage {
        match self {
            SocketAddr::V4(addr) => addr as *const _ as _,
            SocketAddr::V6(addr) => addr as *const _ as _,
        }
    }

    /// Creates a `SocketAddr` from a C `struct sockaddr_storage`.
    /// The function consumes the `struct sockaddr_storage`.
    /// Used in the kernel functions that return a socket address.
    ///
    /// # Panics
    /// Panics if the address family of the `struct sockaddr_storage` is invalid.
    /// This should never happen.
    /// If it does, it is likely because of an invalid pointer.
    pub(crate) fn try_from_raw(sockaddr: SocketAddrStorage) -> Result<Self> {
        match sockaddr.family()? {
            AddressFamily::Inet => Ok(SocketAddr::V4(sockaddr.into())),
            AddressFamily::Inet6 => Ok(SocketAddr::V6(sockaddr.into())),
            _ => Err(code::EINVAL),
        }
    }
}

impl From<SocketAddrV4> for SocketAddr {
    fn from(value: SocketAddrV4) -> Self {
        SocketAddr::V4(value)
    }
}

impl From<SocketAddrV6> for SocketAddr {
    fn from(value: SocketAddrV6) -> Self {
        SocketAddr::V6(value)
    }
}

impl TryFrom<SocketAddr> for SocketAddrV4 {
    type Error = Error;

    fn try_from(value: SocketAddr) -> core::result::Result<Self, Self::Error> {
        match value {
            SocketAddr::V4(addr) => Ok(addr),
            _ => Err(Error::from_errno(bindings::EAFNOSUPPORT as _)),
        }
    }
}

impl TryFrom<SocketAddr> for SocketAddrV6 {
    type Error = Error;

    fn try_from(value: SocketAddr) -> core::result::Result<Self, Self::Error> {
        match value {
            SocketAddr::V6(addr) => Ok(addr),
            _ => Err(Error::from_errno(bindings::EAFNOSUPPORT as _)),
        }
    }
}

/// Generic trait for socket addresses.
///
/// The purpose of this trait is:
/// - To force all socket addresses to have a size and an address family.
/// - Force all socket addresses to implement specific built-in traits.
pub trait GenericSocketAddr:
    Sized + Copy + Clone + PartialEq + Eq + PartialOrd + Ord + Hash + Display
{
    /// Returns the size in bytes of the concrete address.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::bindings;
    /// use kernel::net::addr::{GenericSocketAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
    /// assert_eq!(SocketAddrV4::size(), core::mem::size_of::<bindings::sockaddr_in>());
    /// ```
    fn size() -> usize
    where
        Self: Sized,
    {
        core::mem::size_of::<Self>()
    }

    /// Returns the address family of the concrete address.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kernel::net::addr::{GenericSocketAddr, SocketAddrV4};
    /// use kernel::net::AddressFamily;
    /// assert_eq!(SocketAddrV4::family(), AddressFamily::Inet);
    /// ```
    fn family() -> AddressFamily;
}

/// IPv4 socket address.
///
/// Wraps a C `struct sockaddr_in`.
///
/// # Examples
/// ```rust
/// use kernel::bindings;
/// use kernel::net::addr::{GenericSocketAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
/// let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80);
/// assert_eq!(addr.ip(), &Ipv4Addr::new(192, 168, 0, 1));
/// assert_eq!(SocketAddrV4::size(), core::mem::size_of::<bindings::sockaddr_in>());
/// ```
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct SocketAddrV4(pub(crate) bindings::sockaddr_in);

impl SocketAddrV4 {
    /// The maximum length of a IPv4 socket address string representation.
    ///
    /// This is the length of the string representation of the address.
    /// It does not include the null terminator.
    pub const MAX_STRING_LEN: usize = 21;

    /// Creates a new IPv4 socket address from an IP address and a port.
    ///
    /// The port does not need to be in network byte order.
    pub const fn new(addr: Ipv4Addr, port: u16) -> Self {
        Self(bindings::sockaddr_in {
            sin_family: AddressFamily::Inet as _,
            sin_port: port.to_be(),
            sin_addr: addr.0,
            __pad: [0; 8],
        })
    }

    /// Returns a reference to the IP address contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddrV4};
    ///
    /// let ip = Ipv4Addr::new(192, 168, 0, 1);
    /// let addr = SocketAddrV4::new(ip, 80);
    /// assert_eq!(addr.ip(), &ip);
    /// ```
    pub const fn ip(&self) -> &Ipv4Addr {
        // SAFETY: The [Ipv4Addr] is a transparent representation of the C `struct in_addr`,
        // which is the type of `sin_addr`. Therefore, the conversion is safe.
        unsafe { &*(&self.0.sin_addr as *const _ as *const Ipv4Addr) }
    }

    /// Change the IP address contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddrV4};
    ///
    /// let mut addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80);
    /// addr.set_ip(Ipv4Addr::new(192, 168, 0, 2));
    /// assert_eq!(addr.ip(), &Ipv4Addr::new(192, 168, 0, 2));
    /// ```
    pub fn set_ip(&mut self, ip: Ipv4Addr) {
        self.0.sin_addr = ip.0;
    }

    /// Returns the port contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddrV4};
    ///
    /// let addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80);
    /// assert_eq!(addr.port(), 81);
    /// ```
    pub const fn port(&self) -> u16 {
        self.0.sin_port.to_be()
    }

    /// Change the port contained.
    ///
    /// The port does not need to be in network byte order.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddrV4};
    ///
    /// let mut addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 0, 1), 80);
    /// addr.set_port(81);
    /// assert_eq!(addr.port(), 81);
    /// ```
    pub fn set_port(&mut self, port: u16) {
        self.0.sin_port = port.to_be();
    }
}

impl GenericSocketAddr for SocketAddrV4 {
    /// Returns the family of the address.
    ///
    /// # Invariants
    /// The family is always [AddressFamily::Inet].
    fn family() -> AddressFamily {
        AddressFamily::Inet
    }
}

impl PartialEq<SocketAddrV4> for SocketAddrV4 {
    fn eq(&self, other: &SocketAddrV4) -> bool {
        self.ip() == other.ip() && self.port() == other.port()
    }
}

impl Eq for SocketAddrV4 {}

impl Hash for SocketAddrV4 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.ip(), self.port()).hash(state)
    }
}

impl PartialOrd for SocketAddrV4 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SocketAddrV4 {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.ip(), self.port()).cmp(&(other.ip(), other.port()))
    }
}

/// IPv6 socket address.
///
/// Wraps a C `struct sockaddr_in6`.
///
/// # Examples
/// ```rust
/// use kernel::bindings;
/// use kernel::net::addr::{GenericSocketAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
///
/// let addr = SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 80, 0, 0);
/// assert_eq!(addr.ip(), &Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
/// assert_eq!(SocketAddrV6::size(), core::mem::size_of::<bindings::sockaddr_in6>());
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct SocketAddrV6(pub(crate) bindings::sockaddr_in6);

impl SocketAddrV6 {
    /// The maximum length of a IPv6 socket address string representation.
    ///
    /// This is the length of the string representation of the address.
    /// It does not include the null terminator.
    pub const MAX_STRING_LEN: usize = 74;

    /// Creates a new IPv6 socket address from an IP address, a port, a flowinfo and a scope_id.
    /// The port does not need to be in network byte order.
    pub const fn new(addr: Ipv6Addr, port: u16, flowinfo: u32, scope_id: u32) -> Self {
        Self(bindings::sockaddr_in6 {
            sin6_family: AddressFamily::Inet6 as _,
            sin6_port: port.to_be(),
            sin6_flowinfo: flowinfo,
            sin6_addr: addr.0,
            sin6_scope_id: scope_id,
        })
    }

    /// Returns a reference to the IP address contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let ip = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
    /// let addr = SocketAddrV6::new(ip, 80, 0, 0);
    /// assert_eq!(addr.ip(), &ip);
    /// ```
    pub const fn ip(&self) -> &Ipv6Addr {
        // SAFETY: The [Ipv6Addr] is a transparent representation of the C `struct in6_addr`,
        // which is the type of `sin6_addr`. Therefore, the conversion is safe.
        unsafe { &*(&self.0.sin6_addr as *const _ as *const Ipv6Addr) }
    }

    /// Change the IP address contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let ip1 = Ipv6Addr::LOOPBACK;
    /// let ip2 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2);
    /// let mut addr = SocketAddrV6::new(ip1, 80, 0, 0);
    /// addr.set_ip(ip2);
    /// assert_eq!(addr.ip(), &ip2);
    /// ```
    pub fn set_ip(&mut self, addr: Ipv6Addr) {
        self.0.sin6_addr = addr.0;
    }

    /// Returns the port contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let addr = SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 80, 0, 0);
    /// assert_eq!(addr.port(), 80);
    /// ```
    pub const fn port(&self) -> u16 {
        self.0.sin6_port.to_be()
    }

    /// Change the port contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let mut addr = SocketAddrV6::new(Ipv6Addr::LOOPBACK, 80, 0, 0);
    /// addr.set_port(443);
    /// assert_eq!(addr.port(), 443);
    /// ```
    pub fn set_port(&mut self, port: u16) {
        self.0.sin6_port = port.to_be();
    }

    /// Returns the flowinfo contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let addr = SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 80, 0, 0);
    /// assert_eq!(addr.flowinfo(), 0);
    /// ```
    pub const fn flowinfo(&self) -> u32 {
        self.0.sin6_flowinfo as _
    }

    /// Change the flowinfo contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let mut addr = SocketAddrV6::new(Ipv6Addr::LOOPBACK, 80, 0, 0);
    /// addr.set_flowinfo(1);
    /// assert_eq!(addr.flowinfo(), 1);
    /// ```
    pub fn set_flowinfo(&mut self, flowinfo: u32) {
        self.0.sin6_flowinfo = flowinfo;
    }

    /// Returns the scope_id contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let addr = SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 80, 0, 1);
    /// assert_eq!(addr.scope_id(), 1);
    /// ```
    pub const fn scope_id(&self) -> u32 {
        self.0.sin6_scope_id as _
    }

    /// Change the scope_id contained.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let mut addr = SocketAddrV6::new(Ipv6Addr::LOOPBACK, 80, 0, 0);
    /// addr.set_scope_id(1);
    /// assert_eq!(addr.scope_id(), 1);
    /// ```
    pub fn set_scope_id(&mut self, scope_id: u32) {
        self.0.sin6_scope_id = scope_id;
    }
}

impl GenericSocketAddr for SocketAddrV6 {
    /// Returns the family of the address.
    ///
    /// # Invariants
    /// The family is always [AddressFamily::Inet6].
    fn family() -> AddressFamily {
        AddressFamily::Inet6
    }
}

impl PartialEq<SocketAddrV6> for SocketAddrV6 {
    fn eq(&self, other: &SocketAddrV6) -> bool {
        self.ip() == other.ip()
            && self.port() == other.port()
            && self.flowinfo() == other.flowinfo()
            && self.scope_id() == other.scope_id()
    }
}

impl Eq for SocketAddrV6 {}

impl Hash for SocketAddrV6 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.ip(), self.port(), self.flowinfo(), self.scope_id()).hash(state)
    }
}

impl PartialOrd for SocketAddrV6 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SocketAddrV6 {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.ip(), self.port(), self.flowinfo(), self.scope_id()).cmp(&(
            other.ip(),
            other.port(),
            other.flowinfo(),
            other.scope_id(),
        ))
    }
}

/// Create a Socket address from a string.
///
/// This method is a wrapper for the `inet_pton_with_scope` C function, which transforms a string
/// to the specified sockaddr* structure.
fn address_from_string<T: GenericSocketAddr>(src: &str, port: &str, net: &Namespace) -> Result<T> {
    let src = CString::try_from_fmt(fmt!("{}", src))?;
    let port = CString::try_from_fmt(fmt!("{}", port))?;
    let mut addr = MaybeUninit::<T>::zeroed();

    // SAFETY: FFI call, all pointers are valid for the duration of the call.
    //         The address family matches the address structure.
    match unsafe {
        bindings::inet_pton_with_scope(
            net as *const _ as *mut bindings::net as _,
            T::family() as _,
            src.as_ptr() as _,
            port.as_ptr() as _,
            addr.as_mut_ptr() as _,
        )
    } {
        // SAFETY: The address was initialized by the C function.
        //         Whatever was not initialized, e.g. flow info or scope id for ipv6, are zeroed.
        0 => Ok(unsafe { addr.assume_init() }),
        errno => Err(Error::from_errno(errno as _)),
    }
}

/// Write the string representation of the `T` address to the formatter.
///
/// This function is used to implement the `Display` trait for each address.
///
/// The `cfmt` parameter is the C string format used to format the address.
/// For example, the format for an IPv4 address is `"%pI4"`.
///
/// The `BUF_LEN` parameter is the size of the buffer used to format the address, including the null terminator.
///
/// # Safety
/// In order to have a correct output, the `cfmt` parameter must be a valid C string format for the `T` address.
/// Also, the `BUF_LEN` parameter must be at least the length of the string representation of the address.
unsafe fn write_addr<const BUF_LEN: usize, T: Sized>(
    formatter: &mut Formatter<'_>,
    cfmt: &CStr,
    addr: &T,
) -> core::fmt::Result {
    let mut buff = [0u8; BUF_LEN];
    // SAFETY: the buffer is big enough to contain the string representation of the address.
    //         The format is valid for the address.
    let s = match unsafe {
        bindings::snprintf(
            buff.as_mut_ptr() as _,
            BUF_LEN as _,
            cfmt.as_ptr() as _,
            addr as *const T,
        )
    } {
        n if n < 0 => Err(()),

        // the buffer is probably bigger than the actual string: truncate at the first null byte
        _ => buff
            .iter()
            .position(|&c| c == 0)
            // SAFETY: the buffer contains a UTF-8 valid string and contains a single null terminator.
            .map(|i| unsafe { core::str::from_utf8_unchecked(&buff[..i]) })
            .ok_or(()),
    };
    match s {
        Ok(s) => write!(formatter, "{}", s),
        Err(_) => Err(core::fmt::Error),
    }
}

impl Display for Ipv4Addr {
    /// Display the address as a string.
    /// The bytes are in network order.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv4Addr;
    /// use kernel::pr_info;
    ///
    /// let addr = Ipv4Addr::new(192, 168, 0, 1);
    /// pr_info!("{}", addr); // prints "192.168.0.1"
    /// ```
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // SAFETY: MAX_STRING_LEN is the length of 255.255.255.255, the biggest Ipv4Addr string.
        //         +1 for the null terminator.
        unsafe {
            write_addr::<{ Ipv4Addr::MAX_STRING_LEN + 1 }, Ipv4Addr>(f, c_str!("%pI4"), self)
                .map_err(|_| core::fmt::Error)
        }
    }
}

impl Debug for Ipv4Addr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "Ipv4Addr({})", self)
    }
}

impl FromStr for Ipv4Addr {
    type Err = ();

    /// Create a new IPv4 address from a string.
    /// The string must be in the format `a.b.c.d`, where `a`, `b`, `c` and `d` are 8-bit integers.
    ///
    /// # Examples
    /// Valid addresses:
    /// ```rust
    /// use core::str::FromStr;
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let addr = Ipv4Addr::from_str("192.168.0.1");
    /// assert_eq!(addr, Ok(Ipv4Addr::new(192, 168, 0, 1)));
    /// ```
    ///
    /// Invalid addresses:
    /// ```rust
    /// use core::str::FromStr;
    /// use kernel::net::addr::Ipv4Addr;
    ///
    /// let mut addr = Ipv4Addr::from_str("invalid");
    /// assert_eq!(addr, Err(()));
    ///
    /// addr = Ipv4Addr::from_str("280.168.0.1");
    /// assert_eq!(addr, Err(()));
    ///
    /// addr = Ipv4Addr::from_str("0.0.0.0.0");
    /// assert_eq!(addr, Err(()));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut buffer = [0u8; 4];
        // SAFETY: FFI call,
        //         there is no need to construct a NULL-terminated string, as the length is passed.
        match unsafe {
            bindings::in4_pton(
                s.as_ptr() as *const _,
                s.len() as _,
                buffer.as_mut_ptr() as _,
                -1,
                ptr::null_mut(),
            )
        } {
            1 => Ok(Ipv4Addr::from(buffer)),
            _ => Err(()),
        }
    }
}

impl Display for Ipv6Addr {
    /// Display the address as a string.
    /// The bytes are in network order.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::Ipv6Addr;
    /// use kernel::pr_info;
    ///
    /// let addr = Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334);
    /// pr_info!("{}", addr); // prints "2001:db8:85a3::8a2e:370:7334"
    /// ```
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // SAFETY: MAX_STRING_LEN is the length of ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff, the biggest Ipv6Addr string.
        unsafe {
            write_addr::<{ Ipv6Addr::MAX_STRING_LEN + 1 }, Ipv6Addr>(f, c_str!("%pI6c"), self)
        }
    }
}

impl Debug for Ipv6Addr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "Ipv6Addr({})", self)
    }
}

impl FromStr for Ipv6Addr {
    type Err = ();

    /// Create a new IPv6 address from a string.
    ///
    /// The address must follow the format described in [RFC 4291](https://tools.ietf.org/html/rfc4291#section-2.2).
    ///
    /// # Examples
    /// Valid addresses:
    /// ```rust
    /// use core::str::FromStr;
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let addr = Ipv6Addr::from_str("2001:db8:85a3:0:0:8a2e:370:7334").unwrap();
    /// assert_eq!(addr, Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334));
    /// ```
    ///
    /// Invalid addresses:
    /// ```rust
    /// use core::str::FromStr;
    /// use kernel::net::addr::Ipv6Addr;
    ///
    /// let mut addr = Ipv6Addr::from_str("invalid");
    /// assert_eq!(addr, Err(()));
    ///
    /// addr = Ipv6Addr::from_str("2001:db8:85a3:0:0:8a2e:370:7334:1234");
    /// assert_eq!(addr, Err(()));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut buffer = [0u8; 16];
        // SAFETY: FFI call,
        //         there is no need to construct a NULL-terminated string, as the length is passed.
        match unsafe {
            bindings::in6_pton(
                s.as_ptr() as _,
                s.len() as _,
                buffer.as_mut_ptr() as _,
                -1,
                ptr::null_mut(),
            )
        } {
            1 => Ok(Ipv6Addr::from(buffer)),
            _ => Err(()),
        }
    }
}

impl Display for SocketAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            SocketAddr::V4(addr) => Display::fmt(addr, f),
            SocketAddr::V6(addr) => Display::fmt(addr, f),
        }
    }
}

impl Debug for SocketAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "SocketAddr({})", self)
    }
}

impl FromStr for SocketAddr {
    type Err = Error;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        let funcs = [
            |s| SocketAddrV4::from_str(s).map(SocketAddr::V4),
            |s| SocketAddrV6::from_str(s).map(SocketAddr::V6),
        ];

        funcs.iter().find_map(|f| f(s).ok()).ok_or(code::EINVAL)
    }
}

impl Display for SocketAddrV4 {
    /// Display the address as a string.
    ///
    /// The output is of the form `address:port`, where `address` is the IP address in dotted
    /// decimal notation, and `port` is the port number.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::SocketAddrV4;
    /// use kernel::pr_info;
    ///
    /// let addr = SocketAddrV4::from_str("1.2.3.4:5678").unwrap();
    /// pr_info!("{}", addr); // prints "1.2.3.4:5678"
    /// ```
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // SAFETY: MAX_STRING_LEN is the length of 255.255.255.255:12345, the biggest SocketAddrV4 string.
        unsafe {
            write_addr::<{ SocketAddrV4::MAX_STRING_LEN + 1 }, SocketAddrV4>(
                f,
                c_str!("%pISpc"),
                self,
            )
        }
    }
}

impl Debug for SocketAddrV4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "SocketAddrV4({})", self)
    }
}

impl FromStr for SocketAddrV4 {
    type Err = Error;

    /// Parses a string as an IPv4 socket address.
    ///
    /// The string must be in the form `a.b.c.d:p`, where `a`, `b`, `c`, `d` are the four
    /// components of the IPv4 address, and `p` is the port.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv4Addr, SocketAddrV4};
    ///
    /// // valid
    /// let addr = SocketAddrV4::from_str("192.168.1.0:80").unwrap();
    /// assert_eq!(addr.ip(), &Ipv4Addr::new(192, 168, 1, 0));
    /// assert_eq!(addr.port(), 80);
    ///
    /// // invalid
    /// assert!(SocketAddrV4::from_str("192.168:800:80").is_err());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr, port) = s.split_once(':').ok_or(code::EINVAL)?;
        address_from_string(addr, port, init_net())
    }
}

impl Display for SocketAddrV6 {
    /// Display the address as a string.
    ///
    /// The output string is of the form `[addr]:port`, where `addr` is an IPv6 address and `port`
    /// is a port number.
    ///
    /// Flow info and scope ID are not supported and are excluded from the output.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// let addr = SocketAddrV6::from_str("[::1]:80").unwrap();
    /// pr_info!("{}", addr);  // prints "[::1]:80"
    /// ```
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        // SAFETY: MAX_STRING_LEN is big enough to hold the biggest SocketAddrV6 string.
        unsafe {
            write_addr::<{ SocketAddrV6::MAX_STRING_LEN + 1 }, SocketAddrV6>(
                f,
                c_str!("%pISpc"),
                self,
            )
        }
    }
}

impl Debug for SocketAddrV6 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "SocketAddrV6({})", self)
    }
}

impl FromStr for SocketAddrV6 {
    type Err = Error;

    /// Parses a string as an IPv6 socket address.
    ///
    /// The given string must be of the form `[addr]:port`, where `addr` is an IPv6 address and
    /// `port` is a port number.
    ///
    /// Flow info and scope ID are not supported.
    ///
    /// # Examples
    /// ```rust
    /// use kernel::net::addr::{Ipv6Addr, SocketAddrV6};
    ///
    /// // valid
    /// let addr = SocketAddrV6::from_str("[2001:db8:85a3::8a2e:370:7334]:80").unwrap();
    /// assert_eq!(addr.ip(), &Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0x0000, 0x0000, 0x8a2e, 0x0370, 0x7334));
    /// assert_eq!(addr.port(), 80);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr, port) = s.rsplit_once(':').ok_or(code::EINVAL)?;
        let address = addr.trim_start_matches('[').trim_end_matches(']');
        address_from_string(address, port, init_net())
    }
}
