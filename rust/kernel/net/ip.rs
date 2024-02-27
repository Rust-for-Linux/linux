// SPDX-License-Identifier: GPL-2.0

//! IP protocol definitions.
//!
//! This module contains the kernel structures and functions related to IP protocols.
//!
//! C headers:
//! - [`include/linux/in.h`](../../../../include/linux/in.h)
//! - [`include/linux/ip.h`](../../../../include/linux/ip.h)
//! - [`include/uapi/linux/ip.h`](../../../../include/uapi/linux/ip.h)

/// The Ip protocol.
///
/// See [`tools/include/uapi/linux/in.h`](../../../../tools/include/uapi/linux/in.h)
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum IpProtocol {
    /// Dummy protocol for TCP
    Ip = bindings::IPPROTO_IP as isize,
    /// Internet Control Message Protocol
    Icmp = bindings::IPPROTO_ICMP as isize,
    /// Internet Group Management Protocol
    Igmp = bindings::IPPROTO_IGMP as isize,
    /// IPIP tunnels (older KA9Q tunnels use 94)
    IpIp = bindings::IPPROTO_IPIP as isize,
    /// Transmission Control Protocol
    Tcp = bindings::IPPROTO_TCP as isize,
    /// Exterior Gateway Protocol
    Egp = bindings::IPPROTO_EGP as isize,
    /// PUP protocol
    Pup = bindings::IPPROTO_PUP as isize,
    /// User Datagram Protocol
    Udp = bindings::IPPROTO_UDP as isize,
    /// XNS Idp protocol
    Idp = bindings::IPPROTO_IDP as isize,
    /// SO Transport Protocol Class 4
    Tp = bindings::IPPROTO_TP as isize,
    /// Datagram Congestion Control Protocol
    Dccp = bindings::IPPROTO_DCCP as isize,
    /// Ipv6-in-Ipv4 tunnelling
    Ipv6 = bindings::IPPROTO_IPV6 as isize,
    /// Rsvp Protocol
    Rsvp = bindings::IPPROTO_RSVP as isize,
    /// Cisco GRE tunnels (rfc 1701,1702)
    Gre = bindings::IPPROTO_GRE as isize,
    /// Encapsulation Security Payload protocol
    Esp = bindings::IPPROTO_ESP as isize,
    /// Authentication Header protocol
    Ah = bindings::IPPROTO_AH as isize,
    /// Multicast Transport Protocol
    Mtp = bindings::IPPROTO_MTP as isize,
    /// Ip option pseudo header for BEET
    Beetph = bindings::IPPROTO_BEETPH as isize,
    /// Encapsulation Header
    Encap = bindings::IPPROTO_ENCAP as isize,
    /// Protocol Independent Multicast
    Pim = bindings::IPPROTO_PIM as isize,
    /// Compression Header Protocol
    Comp = bindings::IPPROTO_COMP as isize,
    /// Layer 2 Tunnelling Protocol
    L2Tp = bindings::IPPROTO_L2TP as isize,
    /// Stream Control Transport Protocol
    Sctp = bindings::IPPROTO_SCTP as isize,
    /// Udp-Lite (Rfc 3828)
    UdpLite = bindings::IPPROTO_UDPLITE as isize,
    /// Mpls in Ip (Rfc 4023)
    Mpls = bindings::IPPROTO_MPLS as isize,
    /// Ethernet-within-Ipv6 Encapsulation
    Ethernet = bindings::IPPROTO_ETHERNET as isize,
    /// Raw Ip packets
    Raw = bindings::IPPROTO_RAW as isize,
    /// Multipath Tcp connection
    Mptcp = bindings::IPPROTO_MPTCP as isize,
}
