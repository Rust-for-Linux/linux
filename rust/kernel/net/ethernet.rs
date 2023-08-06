// SPDX-License-Identifier: GPL-2.0

//! Ethernet Module
//!
//! C headers: [`include/uapi/linux/if_ether.h`](../../../../include/uapi/linux/if_ether.h)

use crate::bindings;

// IEEE 802.3 Ethernet magic constants
//
// Taken from the original tree at `include/uapi/linux/if_ether.h`.

/// Octets in one ethernet address
pub const ALEN: usize = bindings::ETH_ALEN as usize;

/// Octets in ethernet type field
pub const TLEN: usize = bindings::ETH_TLEN as usize;

/// Total octets in header
pub const HLEN: usize = bindings::ETH_HLEN as usize;

/// Minimal octet count in frame without the FCS
pub const ZLEN: usize = bindings::ETH_ZLEN as usize;

/// Maximal octet count in payload
pub const DATA_LEN: usize = bindings::ETH_DATA_LEN as usize;

/// Maximal octet count in frame without the FCS
pub const FRAME_LEN: usize = bindings::ETH_FRAME_LEN as usize;

/// Octets in the Frame Check Sum (FCS)
pub const FCS_LEN: usize = bindings::ETH_FCS_LEN as usize;

/// Minimal MTU
///
/// RFC791 fixes it to be at 68 octets for IPv4
pub const MIN_MTU: usize = bindings::ETH_MIN_MTU as usize;

/// Maximal MTU
pub const MAX_MTU: usize = bindings::ETH_MAX_MTU as usize;

/// Ethernet Protocol Identifiers
///
/// These were taken from the original tree at `include/uapi/linux/if_ether.h`.
/// Memory-wise, they are represented as machine-endian `u16`. That size is
/// convenient because it corresponds to the ethertype size in Ethernet II
/// headers.
#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum Proto {
    /// Ethernet loopback protocol
    Loop = bindings::ETH_P_LOOP as u16,
    /// Xerox PUP Packet
    Pup = bindings::ETH_P_PUP as u16,
    /// Xexor PUP Address Transfer packet
    PupAddrTrans = bindings::ETH_P_PUPAT as u16,
    /// TSN (IEEE 1722) packet
    Tsn = bindings::ETH_P_TSN as u16,
    /// ERSPAN version 2 (type III)
    Erspan2 = bindings::ETH_P_ERSPAN2 as u16,
    /// Internet Protocol v4 packet
    Ip = bindings::ETH_P_IP as u16,
    /// CCITT X.25
    X25 = bindings::ETH_P_X25 as u16,
    /// Address resolution protocol
    Arp = bindings::ETH_P_ARP as u16,
    /// G8BPQ AX.25 Ethernet Packet *[ NOT AN OFFICIALLY REGISTERED ID ]*
    Bpq = bindings::ETH_P_BPQ as u16,
    /// Xerox IEEE802.3 PUP packet
    IeeePup = bindings::ETH_P_IEEEPUP as u16,
    /// Xerox IEEE802.3 PUP Address Transfer packet
    IeeePupAddrTrans = bindings::ETH_P_IEEEPUPAT as u16,
    /// B.A.T.M.A.N. - advanced packet *[ NOT AN OFFICIALLY REGISTERED ID ]*
    Batman = bindings::ETH_P_BATMAN as u16,

    /// DEC Assigned Protocol
    Dec = bindings::ETH_P_DEC as u16,
    /// DEC DNA Dump/Load
    DNADumpLoad = bindings::ETH_P_DNA_DL as u16,
    /// DEC DNA Remote Control
    DNARemoteControl = bindings::ETH_P_DNA_RC as u16,
    /// DEC DNA Routing
    DNARouting = bindings::ETH_P_DNA_RT as u16,
    /// DEC LAT
    Lat = bindings::ETH_P_LAT as u16,
    /// DEC Diagnostics
    Diag = bindings::ETH_P_DIAG as u16,
    /// DEC Customer Use
    Cust = bindings::ETH_P_CUST as u16,
    /// DEC Systems Comms Arch
    Sca = bindings::ETH_P_SCA as u16,

    /// Trans Ethernet Bridging
    Teb = bindings::ETH_P_TEB as u16,
    /// Reverse Address Resolution Packet
    Rarp = bindings::ETH_P_RARP as u16,

    /// Appletalk DDP
    ATalk = bindings::ETH_P_ATALK as u16,
    /// Appletakl AARP
    AArp = bindings::ETH_P_AARP as u16,

    /// 802.1Q VLAN Extended Header
    IEEE8021Q = bindings::ETH_P_8021Q as u16,
    /// ERSPAN Type II
    Erspan = bindings::ETH_P_ERSPAN as u16,
    /// IPX over IDX
    Ipx = bindings::ETH_P_IPX as u16,
    /// Internet Protocol v6
    IpV6 = bindings::ETH_P_IPV6 as u16,

    /// IEEE Pause frames. See 802.3 31B
    Pause = bindings::ETH_P_PAUSE as u16,
    /// Slow protocol, see 802.3ad 43B
    Slow = bindings::ETH_P_SLOW as u16,

    /// Web Cache Coordination Protocol
    /// Defined in draft-wilson-wrec-wccp-v2-00.txt
    Wccp = bindings::ETH_P_WCCP as u16,

    /// MPLS Unicast Traffic
    MplsUnicast = bindings::ETH_P_MPLS_UC as u16,
    /// MPLS Multicast Traffic
    MplsMulticast = bindings::ETH_P_MPLS_MC as u16,

    /// Multi-Protocol over ATM
    AtmMpOa = bindings::ETH_P_ATMMPOA as u16,

    /// PPPoE Discovery Message
    PppDisc = bindings::ETH_P_PPP_DISC as u16,
    /// PPPoE Session Messages
    PppSes = bindings::ETH_P_PPP_SES as u16,

    /// HPNA, wlan link-local tunnel
    LinkCtl = bindings::ETH_P_LINK_CTL as u16,
    /// Frame-based ATM Transport over Ethernet
    AtmFate = bindings::ETH_P_ATMFATE as u16,
    /// Port Access Entity (IEEE 802.1X)
    Pae = bindings::ETH_P_PAE as u16,
    /// PROFINET
    Profinet = bindings::ETH_P_PROFINET as u16,
    /// Multiple Proprietary Protocols
    Realtek = bindings::ETH_P_REALTEK as u16,
    /// ATA Over Ethernet
    Aoe = bindings::ETH_P_AOE as u16,
    /// EtherCat
    EtherCat = bindings::ETH_P_ETHERCAT as u16,
    /// 802.1ad Service VLAN
    _8021ad = bindings::ETH_P_8021AD as u16,
    /// 802.1 Local Experimental 1
    _802Ex1 = bindings::ETH_P_802_EX1 as u16,
    /// 802.11 Pre Authentication
    PreAuth = bindings::ETH_P_PREAUTH as u16,
    /// TIPC
    Tipc = bindings::ETH_P_TIPC as u16,
    /// Link Layer Discovery Protocol (LLDP)
    Lldp = bindings::ETH_P_LLDP as u16,
    /// Media Redundancy Protocol
    Mrp = bindings::ETH_P_MRP as u16,
    /// 802.1ae MACSec
    MacSec = bindings::ETH_P_MACSEC as u16,
    /// 802.1ah Backbone Service Tag
    _8021ah = bindings::ETH_P_8021AH as u16,
    /// 802.1Q Multiple Vlan Registration Protocol
    Mvrp = bindings::ETH_P_MVRP as u16,
    /// IEEE 1588 Timesync
    _1588 = bindings::ETH_P_1588 as u16,
    /// NCSI Protocol
    Ncsi = bindings::ETH_P_NCSI as u16,
    /// IEC 62439-3 PRP/HSRv0
    Prp = bindings::ETH_P_PRP as u16,
    /// Connectivity Fault Management
    Cfm = bindings::ETH_P_CFM as u16,
    /// Fiber Channel over Ethernet
    FcoE = bindings::ETH_P_FCOE as u16,
    /// InfiniBand over Ethernet
    IboE = bindings::ETH_P_IBOE as u16,
    /// TDLS
    Tdls = bindings::ETH_P_TDLS as u16,
    /// FCoE Initialization protocol
    Fip = bindings::ETH_P_FIP as u16,
    /// 802.21 Media Independant Handover Protocol
    _80221 = bindings::ETH_P_80221 as u16,
    /// IEC 62439-3 HSRv1
    Hsr = bindings::ETH_P_HSR as u16,
    /// Network Service Header
    Nsh = bindings::ETH_P_NSH as u16,
    /// Ethernet Loopback Packet (per IEEE 802.3)
    Loopback = bindings::ETH_P_LOOPBACK as u16,

    /// Deprecated QinQ VLAN 1 [ NOT AN OFFICIALLY REGISTERED ID ]
    QinQ1 = bindings::ETH_P_QINQ1 as u16,
    /// Deprecated QinQ VLAN 2 [ NOT AN OFFICIALLY REGISTERED ID ]
    QinQ2 = bindings::ETH_P_QINQ2 as u16,
    /// Deprecated QinQ VLAN 3 [ NOT AN OFFICIALLY REGISTERED ID ]
    QinQ3 = bindings::ETH_P_QINQ3 as u16,

    /// Ethertype DSA [ NOT AN OFFICIALLY REGISTERED ID ]
    Edsa = bindings::ETH_P_EDSA as u16,
    /// Fake VLAN Header Tag for DSA [ NOT AN OFFICIALLY REGISTERED ID ]
    Dsa8021Q = bindings::ETH_P_DSA_8021Q as u16,
    /// A5PSW Tag Value [ NOT AN OFFICIALLY REGISTERED ID ]
    DsaA5Psw = bindings::ETH_P_DSA_A5PSW as u16,
    /// ForCES inter-FE LFB type
    Ife = bindings::ETH_P_IFE as u16,
    /// IBM af_iucv [ NOT AN OFFICIALLY REGISTERED ID ]
    AfIucv = bindings::ETH_P_AF_IUCV as u16,
    /// Minimal Ethernet II protocol value
    /// Any protocol value below that is 802.3
    Min8023 = bindings::ETH_P_802_3_MIN as u16,

    // Starting here it's just non DIX types.
    // They won't clash for 1500 types.
    /// Dummy type for 802.3 frames
    _8023 = bindings::ETH_P_802_3 as u16,
    /// Dummy protocol id for AX25
    Ax25 = bindings::ETH_P_AX25 as u16,
    /// Every packet (be careful!)
    All = bindings::ETH_P_ALL as u16,
    /// 802.2 frames
    _8022 = bindings::ETH_P_802_2 as u16,
    /// Internal only
    Snap = bindings::ETH_P_SNAP as u16,
    /// DEC DDCMP: Internal Only
    DDcmp = bindings::ETH_P_DDCMP as u16,
    /// Dummy type for WAN PPP frames
    WanPpp = bindings::ETH_P_WAN_PPP as u16,
    /// Dummy type for PPP MP frames
    PppMp = bindings::ETH_P_PPP_MP as u16,
    /// Localtalk pseudo type
    LocalTalk = bindings::ETH_P_LOCALTALK as u16,
    /// CAN: Controlled Area Network
    Can = bindings::ETH_P_CAN as u16,
    /// CANFD: CAN Flexible Data Rate
    CanFd = bindings::ETH_P_CANFD as u16,
    /// CANXL: CAN eXtended Frame Length
    CanXl = bindings::ETH_P_CANXL as u16,
    /// Dummy type for Atalk over PPP
    PppTalk = bindings::ETH_P_PPPTALK as u16,
    /// 802.2 frames
    Tr8022 = bindings::ETH_P_TR_802_2 as u16,
    /// Mobitex (kaz@cafe.net)
    Mobitex = bindings::ETH_P_MOBITEX as u16,
    /// Card specific control frames
    Control = bindings::ETH_P_CONTROL as u16,
    /// Linux IrDA
    Irda = bindings::ETH_P_IRDA as u16,
    /// Acorn Econet
    Econet = bindings::ETH_P_ECONET as u16,
    /// HDLC frames
    Hdlc = bindings::ETH_P_HDLC as u16,
    /// 1A for ArcNet :-)
    ArcNet = bindings::ETH_P_ARCNET as u16,
    /// Distributed Switch Arch.
    Dsa = bindings::ETH_P_DSA as u16,
    /// Trailer Switch Tagging as u16,
    Trailer = bindings::ETH_P_TRAILER as u16,
    /// Nokia Phonet frames
    Phonet = bindings::ETH_P_PHONET as u16,
    /// IEEE802.15.4 frame
    IEEE802154 = bindings::ETH_P_IEEE802154 as u16,
    /// ST-Ericsson CAIF protocol
    Caif = bindings::ETH_P_CAIF as u16,
    /// Multiplexed DSA protocol
    XDsa = bindings::ETH_P_XDSA as u16,
    /// Qalcomm Multiplexing and Aggregation Protocol (MAP)
    Map = bindings::ETH_P_MAP as u16,
    /// Management Component Transport Protocol packets
    Mctp = bindings::ETH_P_MCTP as u16,
}

impl From<Proto> for u16 {
    fn from(p: Proto) -> u16 {
        p as u16
    }
}

/// Unknown Ethernet Protocol Number Error
#[derive(Clone, Copy, Debug)]
pub struct UnknownProtoError(u16);

impl TryFrom<u16> for Proto {
    type Error = UnknownProtoError;
    fn try_from(u: u16) -> core::result::Result<Self, Self::Error> {
        match u32::from(u) {
            bindings::ETH_P_LOOP => Ok(Self::Loop),
            bindings::ETH_P_PUP => Ok(Self::Pup),
            bindings::ETH_P_PUPAT => Ok(Self::PupAddrTrans),
            bindings::ETH_P_TSN => Ok(Self::Tsn),
            bindings::ETH_P_ERSPAN2 => Ok(Self::Erspan2),
            bindings::ETH_P_IP => Ok(Self::Ip),
            bindings::ETH_P_X25 => Ok(Self::X25),
            bindings::ETH_P_ARP => Ok(Self::Arp),
            bindings::ETH_P_BPQ => Ok(Self::Bpq),
            bindings::ETH_P_IEEEPUP => Ok(Self::IeeePup),
            bindings::ETH_P_IEEEPUPAT => Ok(Self::IeeePupAddrTrans),
            bindings::ETH_P_BATMAN => Ok(Self::Batman),
            bindings::ETH_P_DEC => Ok(Self::Dec),
            bindings::ETH_P_DNA_DL => Ok(Self::DNADumpLoad),
            bindings::ETH_P_DNA_RC => Ok(Self::DNARemoteControl),
            bindings::ETH_P_DNA_RT => Ok(Self::DNARouting),
            bindings::ETH_P_LAT => Ok(Self::Lat),
            bindings::ETH_P_DIAG => Ok(Self::Diag),
            bindings::ETH_P_CUST => Ok(Self::Cust),
            bindings::ETH_P_SCA => Ok(Self::Sca),
            bindings::ETH_P_TEB => Ok(Self::Teb),
            bindings::ETH_P_RARP => Ok(Self::Rarp),
            bindings::ETH_P_ATALK => Ok(Self::ATalk),
            bindings::ETH_P_AARP => Ok(Self::AArp),
            bindings::ETH_P_8021Q => Ok(Self::IEEE8021Q),
            bindings::ETH_P_ERSPAN => Ok(Self::Erspan),
            bindings::ETH_P_IPX => Ok(Self::Ipx),
            bindings::ETH_P_IPV6 => Ok(Self::IpV6),
            bindings::ETH_P_PAUSE => Ok(Self::Pause),
            bindings::ETH_P_SLOW => Ok(Self::Slow),
            bindings::ETH_P_WCCP => Ok(Self::Wccp),
            bindings::ETH_P_MPLS_UC => Ok(Self::MplsUnicast),
            bindings::ETH_P_MPLS_MC => Ok(Self::MplsMulticast),
            bindings::ETH_P_ATMMPOA => Ok(Self::AtmMpOa),
            bindings::ETH_P_PPP_DISC => Ok(Self::PppDisc),
            bindings::ETH_P_PPP_SES => Ok(Self::PppSes),
            bindings::ETH_P_LINK_CTL => Ok(Self::LinkCtl),
            bindings::ETH_P_ATMFATE => Ok(Self::AtmFate),
            bindings::ETH_P_PAE => Ok(Self::Pae),
            bindings::ETH_P_PROFINET => Ok(Self::Profinet),
            bindings::ETH_P_REALTEK => Ok(Self::Realtek),
            bindings::ETH_P_AOE => Ok(Self::Aoe),
            bindings::ETH_P_ETHERCAT => Ok(Self::EtherCat),
            bindings::ETH_P_8021AD => Ok(Self::_8021ad),
            bindings::ETH_P_802_EX1 => Ok(Self::_802Ex1),
            bindings::ETH_P_PREAUTH => Ok(Self::PreAuth),
            bindings::ETH_P_TIPC => Ok(Self::Tipc),
            bindings::ETH_P_LLDP => Ok(Self::Lldp),
            bindings::ETH_P_MRP => Ok(Self::Mrp),
            bindings::ETH_P_MACSEC => Ok(Self::MacSec),
            bindings::ETH_P_8021AH => Ok(Self::_8021ah),
            bindings::ETH_P_MVRP => Ok(Self::Mvrp),
            bindings::ETH_P_1588 => Ok(Self::_1588),
            bindings::ETH_P_NCSI => Ok(Self::Ncsi),
            bindings::ETH_P_PRP => Ok(Self::Prp),
            bindings::ETH_P_CFM => Ok(Self::Cfm),
            bindings::ETH_P_FCOE => Ok(Self::FcoE),
            bindings::ETH_P_IBOE => Ok(Self::IboE),
            bindings::ETH_P_TDLS => Ok(Self::Tdls),
            bindings::ETH_P_FIP => Ok(Self::Fip),
            bindings::ETH_P_80221 => Ok(Self::_80221),
            bindings::ETH_P_HSR => Ok(Self::Hsr),
            bindings::ETH_P_NSH => Ok(Self::Nsh),
            bindings::ETH_P_LOOPBACK => Ok(Self::Loopback),
            bindings::ETH_P_QINQ1 => Ok(Self::QinQ1),
            bindings::ETH_P_QINQ2 => Ok(Self::QinQ2),
            bindings::ETH_P_QINQ3 => Ok(Self::QinQ3),
            bindings::ETH_P_EDSA => Ok(Self::Edsa),
            bindings::ETH_P_DSA_8021Q => Ok(Self::Dsa8021Q),
            bindings::ETH_P_DSA_A5PSW => Ok(Self::DsaA5Psw),
            bindings::ETH_P_IFE => Ok(Self::Ife),
            bindings::ETH_P_AF_IUCV => Ok(Self::AfIucv),
            bindings::ETH_P_802_3_MIN => Ok(Self::Min8023),
            bindings::ETH_P_802_3 => Ok(Self::_8023),
            bindings::ETH_P_AX25 => Ok(Self::Ax25),
            bindings::ETH_P_ALL => Ok(Self::All),
            bindings::ETH_P_802_2 => Ok(Self::_8022),
            bindings::ETH_P_SNAP => Ok(Self::Snap),
            bindings::ETH_P_DDCMP => Ok(Self::DDcmp),
            bindings::ETH_P_WAN_PPP => Ok(Self::WanPpp),
            bindings::ETH_P_PPP_MP => Ok(Self::PppMp),
            bindings::ETH_P_LOCALTALK => Ok(Self::LocalTalk),
            bindings::ETH_P_CAN => Ok(Self::Can),
            bindings::ETH_P_CANFD => Ok(Self::CanFd),
            bindings::ETH_P_CANXL => Ok(Self::CanXl),
            bindings::ETH_P_PPPTALK => Ok(Self::PppTalk),
            bindings::ETH_P_TR_802_2 => Ok(Self::Tr8022),
            bindings::ETH_P_MOBITEX => Ok(Self::Mobitex),
            bindings::ETH_P_CONTROL => Ok(Self::Control),
            bindings::ETH_P_IRDA => Ok(Self::Irda),
            bindings::ETH_P_ECONET => Ok(Self::Econet),
            bindings::ETH_P_HDLC => Ok(Self::Hdlc),
            bindings::ETH_P_ARCNET => Ok(Self::ArcNet),
            bindings::ETH_P_DSA => Ok(Self::Dsa),
            bindings::ETH_P_TRAILER => Ok(Self::Trailer),
            bindings::ETH_P_PHONET => Ok(Self::Phonet),
            bindings::ETH_P_IEEE802154 => Ok(Self::IEEE802154),
            bindings::ETH_P_CAIF => Ok(Self::Caif),
            bindings::ETH_P_XDSA => Ok(Self::XDsa),
            bindings::ETH_P_MAP => Ok(Self::Map),
            bindings::ETH_P_MCTP => Ok(Self::Mctp),
            _ => Err(UnknownProtoError(u)),
        }
    }
}

/// Ethernet Address Type
#[repr(transparent)]
pub struct Address(pub [u8; ALEN]);

impl Address {
    /// Return the unspecified Ethernet address
    pub const fn unspecified() -> Self {
        Self([0x00; ALEN])
    }

    /// Return the broadcast Ethernet address
    pub const fn broadcast() -> Self {
        Self([0xff; ALEN])
    }
}

/// Ethernet II protocol header
#[repr(transparent)]
pub struct Header(bindings::ethhdr);

impl Header {
    /// Build a new Ethernet header from all necessary information
    ///
    /// This method automatically handles conversion of the ethertype provided
    /// into a network-endian u16.
    pub fn new(dst: Address, src: Address, proto: Proto) -> Self {
        Self(bindings::ethhdr {
            h_dest: dst.0,
            h_source: src.0,
            h_proto: u16::to_be(u16::from(proto)),
        })
    }

    /// Return the source address contained in the header
    pub fn src_hwaddr(&self) -> Address {
        Address(self.0.h_source)
    }

    /// Return the destination address contained in the header
    pub fn dst_hwaddr(&self) -> Address {
        Address(self.0.h_dest)
    }

    /// Return the protocol number contained in the header
    ///
    /// This method does not handle conversion from network endian.
    pub fn proto_number(&self) -> u16 {
        self.0.h_proto
    }
}
