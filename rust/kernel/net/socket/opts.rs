// SPDX-License-Identifier: GPL-2.0

//! Socket options.
//!
//! This module contains the types related to socket options.
//! It is meant to be used together with the [`Socket`](kernel::net::socket::Socket) type.
//!
//! Socket options have more sense in the user space than in the kernel space: the kernel can
//! directly access the socket data structures, so it does not need to use socket options.
//! However, that level of freedom is currently not available in the Rust kernel API; therefore,
//! having socket options is a good compromise.
//!
//! When Rust wrappers for the structures related to the socket (and required by the options,
//! e.g. `tcp_sock`, `inet_sock`, etc.) are available, the socket options will be removed,
//! and substituted by direct methods inside the socket types.

use kernel::bindings;

/// Options level to retrieve and set socket options.
/// See `man 7 socket` for more information.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum OptionsLevel {
    /// IP level socket options.
    /// See `man 7 ip` for more information.
    Ip = bindings::IPPROTO_IP as isize,

    /// Socket level socket options.
    /// See `man 7 socket` for more information.
    Socket = bindings::SOL_SOCKET as isize,

    /// IPv6 level socket options.
    /// See `man 7 ipv6` for more information.
    Ipv6 = bindings::IPPROTO_IPV6 as isize,

    /// Raw level socket options.
    /// See `man 7 raw` for more information.
    Raw = bindings::IPPROTO_RAW as isize,

    /// TCP level socket options.
    /// See `man 7 tcp` for more information.
    Tcp = bindings::IPPROTO_TCP as isize,
}

/// Generic socket option type.
///
/// This trait is implemented by each individual socket option.
///
/// Having socket options as structs instead of enums allows:
/// - Type safety, making sure that the correct type is used for each option.
/// - Read/write enforcement, making sure that only readable options
/// are read and only writable options are written.
pub trait SocketOption {
    /// Rust type of the option value.
    ///
    /// This type is used to store the value of the option.
    /// It is also used to enforce type safety.
    ///
    /// For example, the [`ip::Mtu`] option has a value of type `u32`.
    type Type;

    /// Retrieve the C value of the option.
    ///
    /// This value is used to pass the option to the kernel.
    fn value() -> isize;

    /// Retrieve the level of the option.
    ///
    /// This value is used to pass the option to the kernel.
    fn level() -> OptionsLevel;
}

/// Generic readable socket option type.
///
/// This trait is implemented by each individual readable socket option.
/// Can be combined with [`WritableOption`] to create a readable and writable socket option.
pub trait WritableOption: SocketOption {}

/// Generic writable socket option type.
///
/// This trait is implemented by each individual writable socket option.
/// Can be combined with [`ReadableOption`] to create a readable and writable socket option.
pub trait ReadableOption: SocketOption {}

/// Generates the code for the implementation of a socket option.
///
/// # Parameters
/// * `$opt`: Name of the socket option.
/// * `$value`: C value of the socket option.
/// * `$level`: Level of the socket option, like [`OptionsLevel::Ip`].
/// * `$rtyp`: Rust type of the socket option.
/// * `$($tr:ty),*`: Traits that the socket option implements, like [`WritableOption`].
macro_rules! impl_opt {
    ($(#[$meta:meta])*
    $opt:ident = $value:expr,
    $level:expr,
    unimplemented,
    $($tr:ty),*) => {};

    ($(#[$meta:meta])*
    $opt:ident = $value:expr,
    $level:expr,
    $rtyp:ty,
    $($tr:ty),*) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Default)]
        pub struct $opt;
        impl SocketOption for $opt {
            type Type = $rtyp;
            fn value() -> isize {
                $value as isize
            }
            fn level() -> OptionsLevel {
                $level
            }
        }
        $(
            impl $tr for $opt {}
        )*
    };
}

pub mod ip {
    //! IP socket options.
    use super::{OptionsLevel, ReadableOption, SocketOption, WritableOption};
    use crate::net::addr::Ipv4Addr;

    macro_rules! impl_ip_opt {
        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        unimplemented,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Ip,
                unimplemented,
                $($tr),*
            );
        };

        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        $rtyp:ty,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Ip,
                $rtyp,
                $($tr),*
            );
        };
    }

    impl_ip_opt!(
        /// Join a multicast group.
        ///
        /// C value type: `struct ip_mreqn`.
        AddMembership = bindings::IP_ADD_MEMBERSHIP,
        unimplemented,
        WritableOption
    );
    impl_ip_opt!(
        /// Join a multicast group with source filtering.
        ///
        /// C value type: `struct ip_mreq_source`
        AddSourceMembership = bindings::IP_ADD_SOURCE_MEMBERSHIP,
        unimplemented,
        WritableOption
    );
    impl_ip_opt!(
        /// Don't reserve a port when binding with port number 0.
        ///
        /// C value type: `int`
        BindAddressNoPort = bindings::IP_BIND_ADDRESS_NO_PORT,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Block packets from a specific source.
        ///
        /// C value type: `struct ip_mreq_source`
        BlockSource = bindings::IP_BLOCK_SOURCE,
        unimplemented,
        WritableOption
    );
    impl_ip_opt!(
        /// Leave a multicast group.
        ///
        /// C value type: `struct ip_mreqn`
        DropMembership = bindings::IP_DROP_MEMBERSHIP,
        unimplemented,
        WritableOption
    );
    impl_ip_opt!(
        /// Stop receiving packets from a specific source.
        ///
        /// C value type: `struct ip_mreq_source`
        DropSourceMembership = bindings::IP_DROP_SOURCE_MEMBERSHIP,
        unimplemented,
        WritableOption
    );
    impl_ip_opt!(
        /// Allow binding to a non-local address.
        ///
        /// C value type: `int`
        FreeBind = bindings::IP_FREEBIND,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Receive the IP header with the packet.
        ///
        /// C value type: `int`
        Header = bindings::IP_HDRINCL,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Full-state multicast filtering API.
        ///
        /// C value type: `struct ip_msfilter`
        MsFilter = bindings::IP_MSFILTER,
        unimplemented,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Retrieve the MTU of the socket.
        ///
        /// C value type: `int`
        Mtu = bindings::IP_MTU,
        u32,
        ReadableOption
    );
    impl_ip_opt!(
        /// Discover the MTU of the path to a destination.
        ///
        /// C value type: `int`
        MtuDiscover = bindings::IP_MTU_DISCOVER,
        unimplemented,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Modify delivery policy of messages.
        ///
        /// C value type: `int`
        MulticastAll = bindings::IP_MULTICAST_ALL,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set the interface for outgoing multicast packets.
        ///
        /// C value type: `struct in_addr`
        MulticastInterface = bindings::IP_MULTICAST_IF,
        Ipv4Addr,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set whether multicast packets are looped back to the sender.
        ///
        /// C value type: `int`
        MulticastLoop = bindings::IP_MULTICAST_LOOP,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set the TTL of outgoing multicast packets.
        ///
        /// C value type: `int`
        MulticastTtl = bindings::IP_MULTICAST_TTL,
        u8,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Whether to disable reassembling of fragmented packets.
        ///
        /// C value type: `int`
        NoDefrag = bindings::IP_NODEFRAG,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set the options to be included in outgoing packets.
        ///
        /// C value type: `char *`
        IpOptions = bindings::IP_OPTIONS,
        unimplemented,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Enable receiving security context with the packet.
        ///
        /// C value type: `int`
        PassSec = bindings::IP_PASSSEC,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Enable extended reliable error message passing.
        ///
        /// C value type: `int`
        RecvErr = bindings::IP_RECVERR,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Pass all IP Router Alert messages to this socket.
        ///
        /// C value type: `int`
        RouterAlert = bindings::IP_ROUTER_ALERT,
        bool,
        WritableOption
    );
    impl_ip_opt!(
        /// Set the TOS field of outgoing packets.
        ///
        /// C value type: `int`
        Tos = bindings::IP_TOS,
        u8,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set transparent proxying.
        ///
        /// C value type: `int`
        Transparent = bindings::IP_TRANSPARENT,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Set the TTL of outgoing packets.
        ///
        /// C value type: `int`
        Ttl = bindings::IP_TTL,
        u8,
        ReadableOption,
        WritableOption
    );
    impl_ip_opt!(
        /// Unblock packets from a specific source.
        ///
        /// C value type: `struct ip_mreq_source`
        UnblockSource = bindings::IP_UNBLOCK_SOURCE,
        unimplemented,
        WritableOption
    );
}

pub mod sock {
    //! Socket options.
    use super::*;
    use crate::net::ip::IpProtocol;
    use crate::net::socket::SockType;
    use crate::net::AddressFamily;
    macro_rules! impl_sock_opt {
        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        unimplemented,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Socket,
                unimplemented,
                $($tr),*
            );
        };

        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        $rtyp:ty,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Socket,
                $rtyp,
                $($tr),*
            );
        };
    }

    impl_sock_opt!(
        /// Get whether the socket is accepting connections.
        ///
        /// C value type: `int`
        AcceptConn = bindings::SO_ACCEPTCONN,
        bool,
        ReadableOption
    );

    impl_sock_opt!(
        /// Attach a filter to the socket.
        ///
        /// C value type: `struct sock_fprog`
        AttachFilter = bindings::SO_ATTACH_FILTER,
        unimplemented,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Attach a eBPF program to the socket.
        ///
        /// C value type: `struct sock_fprog`
        AttachBpf = bindings::SO_ATTACH_BPF,
        unimplemented,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Bind the socket to a specific network device.
        ///
        /// C value type: `char *`
        BindToDevice = bindings::SO_BINDTODEVICE,
        &'static str,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Set the broadcast flag on the socket.
        ///
        /// Only valid for datagram sockets.
        ///
        /// C value type: `int`
        Broadcast = bindings::SO_BROADCAST,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Enable BSD compatibility.
        ///
        /// C value type: `int`
        BsdCompatible = bindings::SO_BSDCOMPAT,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Enable socket debugging.
        ///
        /// C value type: `int`
        Debug = bindings::SO_DEBUG,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Remove BPF or eBPF program from the socket.
        ///
        /// The argument is ignored.
        ///
        /// C value type: `int`
        DetachFilter = bindings::SO_DETACH_FILTER,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Get the domain of the socket.
        ///
        /// C value type: `int`
        Domain = bindings::SO_DOMAIN,
        AddressFamily,
        ReadableOption
    );
    impl_sock_opt!(
        /// Get and clear pending errors.
        ///
        /// C value type: `int`
        Error = bindings::SO_ERROR,
        u32,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Only send packets to directly connected peers.
        ///
        /// C value type: `int`
        DontRoute = bindings::SO_DONTROUTE,
        bool,
        ReadableOption,
        WritableOption
    );
    impl_sock_opt!(
        /// Set or get the CPU affinity of a socket.
        ///
        /// C value type: `int`
        IncomingCpu = bindings::SO_INCOMING_CPU,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Enable keep-alive packets.
        ///
        /// C value type: `int`
        KeepAlive = bindings::SO_KEEPALIVE,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the linger timeout.
        ///
        /// C value type: `struct linger`
        Linger = bindings::SO_LINGER,
        Linger,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Prevent changing the filters attached to the socket.
        ///
        /// C value type: `int`
        LockFilter = bindings::SO_LOCK_FILTER,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the mark of the socket.
        ///
        /// C value type: `int`
        Mark = bindings::SO_MARK,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set whether out-of-band data is received in the normal data stream.
        ///
        /// C value type: `int`
        OobInline = bindings::SO_OOBINLINE,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Enable the receiving of SCM credentials.
        ///
        /// C value type: `int`
        PassCred = bindings::SO_PASSCRED,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set the peek offset for MSG_PEEK reads.
        ///
        /// Only valid for UNIX sockets.
        ///
        /// C value type: `int`
        PeekOff = bindings::SO_PEEK_OFF,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the protocol-defined priority for all packets.
        ///
        /// C value type: `int`
        Priority = bindings::SO_PRIORITY,
        u8,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Retrieve the socket protocol
        ///
        /// C value type: `int`
        Protocol = bindings::SO_PROTOCOL,
        IpProtocol,
        ReadableOption
    );

    impl_sock_opt!(
        /// Set or get the receive buffer size.
        ///
        /// C value type: `int`
        RcvBuf = bindings::SO_RCVBUF,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the receive low watermark.
        ///
        /// C value type: `int`
        RcvLowat = bindings::SO_RCVLOWAT,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the receive timeout.
        ///
        /// C value type: `struct timeval`
        RcvTimeo = bindings::SO_RCVTIMEO_NEW,
        unimplemented,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the reuse address flag.
        ///
        /// C value type: `int`
        ReuseAddr = bindings::SO_REUSEADDR,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the reuse port flag.
        ///
        /// C value type: `int`
        ReusePort = bindings::SO_REUSEPORT,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the send buffer size.
        ///
        /// C value type: `int`
        SndBuf = bindings::SO_SNDBUF,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the send timeout.
        ///
        /// C value type: `struct timeval`
        SndTimeo = bindings::SO_SNDTIMEO_NEW,
        unimplemented,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set whether the timestamp control messages are received.
        ///
        /// C value type: `int`
        Timestamp = bindings::SO_TIMESTAMP_NEW,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_sock_opt!(
        /// Set or get the socket type.
        ///
        /// C value type: `int`
        Type = bindings::SO_TYPE,
        SockType,
        ReadableOption
    );
}

pub mod ipv6 {
    //! IPv6 socket options.
    use super::*;
    use crate::net::AddressFamily;
    macro_rules! impl_ipv6_opt {
        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        unimplemented,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Ipv6,
                unimplemented,
                $($tr),*
            );
        };

        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        $rtyp:ty,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Ipv6,
                $rtyp,
                $($tr),*
            );
        };
    }

    impl_ipv6_opt!(
        /// Modify the address family used by the socket.
        ///
        /// C value type: `int`
        AddrForm = bindings::IPV6_ADDRFORM,
        AddressFamily,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Join a multicast group.
        ///
        /// C value type: `struct ipv6_mreq`
        AddMembership = bindings::IPV6_ADD_MEMBERSHIP,
        unimplemented,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Leave a multicast group.
        ///
        /// C value type: `struct ipv6_mreq`
        DropMembership = bindings::IPV6_DROP_MEMBERSHIP,
        unimplemented,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get the MTU of the socket.
        ///
        /// C value type: `int`
        Mtu = bindings::IPV6_MTU,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or retrieve the MTU discovery settings.
        ///
        /// C value type: `int` (macros)
        MtuDiscover = bindings::IPV6_MTU_DISCOVER,
        unimplemented,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get the multicast hop limit.
        ///
        /// Range is -1 to 255.
        ///
        /// C value type: `int`
        MulticastHops = bindings::IPV6_MULTICAST_HOPS,
        i16,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get the multicast interface.
        ///
        /// Only valid for datagram and raw sockets.
        ///
        /// C value type: `int`
        MulticastInterface = bindings::IPV6_MULTICAST_IF,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or read whether multicast packets are looped back
        ///
        /// C value type: `int`
        MulticastLoop = bindings::IPV6_MULTICAST_LOOP,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get whether IPV6_PKTINFO is enabled.
        ///
        /// Only valid for datagram and raw sockets.
        ///
        /// C value type: `int`
        ReceivePktInfo = bindings::IPV6_PKTINFO,
        bool,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get whether IPV6_RTHDR messages are delivered.
        ///
        /// Only valid for raw sockets.
        ///
        /// C value type: `int`
        RouteHdr = bindings::IPV6_RTHDR,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get whether IPV6_DSTOPTS messages are delivered.
        ///
        /// Only valid for datagram and raw sockets.
        ///
        /// C value type: `int`
        DestOptions = bindings::IPV6_DSTOPTS,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get whether IPV6_HOPOPTS messages are delivered.
        ///
        /// Only valid for datagram and raw sockets.
        ///
        /// C value type: `int`
        HopOptions = bindings::IPV6_HOPOPTS,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get whether IPV6_FLOWINFO messages are delivered.
        ///
        /// Only valid for datagram and raw sockets.
        ///
        /// C value type: `int`
        FlowInfo = bindings::IPV6_FLOWINFO,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Enable extended reliable error message reporting.
        ///
        /// C value type: `int`
        RecvErr = bindings::IPV6_RECVERR,
        bool,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Pass all Router Alert enabled messages to the socket.
        ///
        /// Only valid for raw sockets.
        ///
        /// C value type: `int`
        RouterAlert = bindings::IPV6_ROUTER_ALERT,
        bool,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set or get the unicast hop limit.
        ///
        /// Range is -1 to 255.
        ///
        /// C value type: `int`
        UnicastHops = bindings::IPV6_UNICAST_HOPS,
        i16,
        ReadableOption,
        WritableOption
    );

    impl_ipv6_opt!(
        /// Set whether the socket can only send and receive IPv6 packets.
        ///
        /// C value type: `int`
        V6Only = bindings::IPV6_V6ONLY,
        bool,
        ReadableOption,
        WritableOption
    );
}

pub mod raw {
    //! Raw socket options.
    //!
    //! These options are only valid for sockets with type [`SockType::Raw`](kernel::net::socket::SockType::Raw).
    macro_rules! impl_raw_opt {
        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        unimplemented,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Raw,
                unimplemented,
                $($tr),*
            );
        };

        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        $rtyp:ty,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Raw,
                $rtyp,
                $($tr),*
            );
        };
    }

    impl_raw_opt!(
        /// Enable a filter for IPPROTO_ICMP raw sockets.
        /// The filter has a bit set for each ICMP type to be filtered out.
        ///
        /// C value type: `struct icmp_filter`
        Filter = bindings::ICMP_FILTER as isize,
        unimplemented,
        ReadableOption,
        WritableOption
    );
}

pub mod tcp {
    //! TCP socket options.
    //!
    //! These options are only valid for sockets with type [`SockType::Stream`](kernel::net::socket::SockType::Stream)
    //! and protocol [`IpProtocol::Tcp`](kernel::net::ip::IpProtocol::Tcp).
    use super::*;
    macro_rules! impl_tcp_opt {
        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        unimplemented,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Tcp,
                unimplemented,
                $($tr),*
            );
        };

        ($(#[$meta:meta])*
        $opt:ident = $value:expr,
        $rtyp:ty,
        $($tr:ty),*) => {
            impl_opt!(
                $(#[$meta])*
                $opt = $value,
                OptionsLevel::Tcp,
                $rtyp,
                $($tr),*
            );
        };
    }

    impl_tcp_opt!(
        /// Set or get the congestion control algorithm to be used.
        ///
        /// C value type: `char *`
        Congestion = bindings::TCP_CONGESTION,
        unimplemented, // &[u8]? what about lifetime?
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// If true, don't send partial frames.
        ///
        /// C value type: `int`
        Cork = bindings::TCP_CORK,
        bool,
        WritableOption,
        ReadableOption
    );

    impl_tcp_opt!(
        /// Allow a listener to be awakened only when data arrives.
        /// The value is the time to wait for data in milliseconds.
        ///
        /// C value type: `int`
        DeferAccept = bindings::TCP_DEFER_ACCEPT,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Collect information about this socket.
        ///
        /// C value type: `struct tcp_info`
        Info = bindings::TCP_INFO,
        unimplemented,
        ReadableOption
    );

    impl_tcp_opt!(
        /// Set or get maximum number of keepalive probes to send.
        ///
        /// C value type: `int`
        KeepCount = bindings::TCP_KEEPCNT,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get the time in seconds to idle before sending keepalive probes.
        ///
        /// C value type: `int`
        KeepIdle = bindings::TCP_KEEPIDLE,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get the time in seconds between keepalive probes.
        ///
        /// C value type: `int`
        KeepInterval = bindings::TCP_KEEPINTVL,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get the lifetime or orphaned FIN_WAIT2 sockets.
        ///
        /// C value type: `int`
        Linger2 = bindings::TCP_LINGER2,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get the maximum segment size for outgoing TCP packets.
        ///
        /// C value type: `int`
        MaxSeg = bindings::TCP_MAXSEG,
        i32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// If true, Nagle algorithm is disabled, i.e. segments are send as soon as possible.
        ///
        /// C value type: `int`
        NoDelay = bindings::TCP_NODELAY,
        bool,
        WritableOption,
        ReadableOption
    );

    impl_tcp_opt!(
        /// Set or get whether QuickAck mode is on.
        /// If true, ACKs are sent immediately, rather than delayed.
        ///
        /// C value type: `int`
        QuickAck = bindings::TCP_QUICKACK,
        bool,
        WritableOption,
        ReadableOption
    );

    impl_tcp_opt!(
        /// Set or get the number of SYN retransmits before the connection is dropped.
        ///
        /// C value type: `int`
        SynCount = bindings::TCP_SYNCNT,
        u8,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get how long sent packets can remain unacknowledged before timing out.
        /// The value is in milliseconds; 0 means to use the system default.
        ///
        /// C value type: `unsigned int`
        UserTimeout = bindings::TCP_USER_TIMEOUT,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Set or get the maximum window size for TCP sockets.
        ///
        /// C value type: `int`
        WindowClamp = bindings::TCP_WINDOW_CLAMP,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Enable Fast Open on the listener socket (RFC 7413).
        /// The value is the maximum length of pending SYNs.
        ///
        /// C value type: `int`
        FastOpen = bindings::TCP_FASTOPEN,
        u32,
        ReadableOption,
        WritableOption
    );

    impl_tcp_opt!(
        /// Enable Fast Open on the client socket (RFC 7413).
        ///
        /// C value type: `int`
        FastOpenConnect = bindings::TCP_FASTOPEN_CONNECT,
        bool,
        ReadableOption,
        WritableOption
    );
}

/// Linger structure to set and get the [sock::Linger] option.
/// This is a wrapper around the C struct `linger`.
#[repr(transparent)]
pub struct Linger(bindings::linger);

impl Linger {
    /// Create a "on" Linger object with the given linger time.
    /// This is equivalent to `linger { l_onoff: 1, l_linger: linger_time }`.
    /// The linger time is in seconds.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::opts::Linger;
    /// let linger = Linger::on(10);
    /// assert!(linger.is_on());
    /// assert_eq!(linger.linger_time(), 10);
    pub fn on(linger: i32) -> Self {
        Linger(bindings::linger {
            l_onoff: 1 as _,
            l_linger: linger as _,
        })
    }

    /// Create an "off" Linger object.
    /// This is equivalent to `linger { l_onoff: 0, l_linger: 0 }`.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::opts::Linger;
    /// let linger = Linger::off();
    /// assert!(!linger.is_on());
    pub fn off() -> Self {
        Linger(bindings::linger {
            l_onoff: 0 as _,
            l_linger: 0 as _,
        })
    }

    /// Get whether the linger option is on.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::opts::Linger;
    /// let linger = Linger::on(10);
    /// assert!(linger.is_on());
    /// ```
    ///
    /// ```
    /// use kernel::net::socket::opts::Linger;
    /// let linger = Linger::off();
    /// assert!(!linger.is_on());
    /// ```
    pub fn is_on(&self) -> bool {
        self.0.l_onoff != 0
    }

    /// Get the linger time in seconds.
    /// If the linger option is off, this will return 0.
    ///
    /// # Example
    /// ```
    /// use kernel::net::socket::opts::Linger;
    /// let linger = Linger::on(10);
    /// assert_eq!(linger.linger_time(), 10);
    /// ```
    pub fn linger_time(&self) -> i32 {
        self.0.l_linger as _
    }
}
