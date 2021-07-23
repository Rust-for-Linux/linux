use crate::{bindings, Error};
use core::convert::{From, TryFrom};
use core::ops::{Add, AddAssign, Sub, SubAssign};

/// Holds multiple flags to give to an interface via [`super::NetDevice::add_flag`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Features(u64);

impl Features {
    /// Create new Flag with value `0`.
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Add flag to Self.
    #[inline]
    pub fn insert(&mut self, flag: u64) {
        self.0 |= flag;
    }

    /// Remove the given flag from Self.
    #[inline]
    pub fn remove(&mut self, flag: u64) {
        self.0 &= !(flag);
    }
}

impl Add for Features {
    type Output = Self;

    #[inline]
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl Add<u64> for Features {
    type Output = Self;

    #[inline]
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 | rhs)
    }
}

impl Sub for Features {
    type Output = Self;

    #[inline]
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 & !rhs.0)
    }
}

impl Sub<u64> for Features {
    type Output = Self;

    #[inline]
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn sub(self, rhs: u64) -> Self::Output {
        Self(self.0 & !rhs)
    }
}

impl AddAssign for Features {
    #[inline]
    #[allow(clippy::suspicious_op_assign_impl)]
    fn add_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0
    }
}

impl AddAssign<u64> for Features {
    #[inline]
    #[allow(clippy::suspicious_op_assign_impl)]
    fn add_assign(&mut self, rhs: u64) {
        self.0 |= rhs
    }
}

impl SubAssign for Features {
    #[inline]
    #[allow(clippy::suspicious_op_assign_impl)]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 &= !rhs.0
    }
}

impl SubAssign<u64> for Features {
    #[inline]
    #[allow(clippy::suspicious_op_assign_impl)]
    fn sub_assign(&mut self, rhs: u64) {
        self.0 &= !rhs
    }
}

impl TryFrom<u64> for Features {
    type Error = Error;

    #[inline]
    fn try_from(flags: u64) -> Result<Self, Self::Error> {
        Ok(Self(flags))
    }
}

impl From<Features> for u64 {
    #[inline]
    fn from(flag: Features) -> Self {
        flag.0
    }
}

macro_rules! _netif_f {
    ($name:ident, $binding:ident) => {
        #[doc = concat!("[`Features`] flag for `", stringify!($binding), "`")]
        pub const $name: Features = Features(1u64 << $crate::bindings::$binding);
    };
}

macro_rules! _netif_f_sum {
        ($name:ident, $($f:ident),+) => {
            #[doc = concat!("[`Features`] flag for `NETIF_F_", stringify!($name), "`")]
            pub const $name: Features = Features($(Features::$f.0)|*);
        };
    }

impl Features {
    _netif_f!(SG, NETIF_F_SG_BIT);
    _netif_f!(FRAGLIST, NETIF_F_FRAGLIST_BIT);
    _netif_f!(TSO, NETIF_F_TSO_BIT);
    _netif_f!(TSO6, NETIF_F_TSO6_BIT);
    _netif_f!(TSO_ECN, NETIF_F_TSO_ECN_BIT);
    _netif_f!(TSO_MANGLEID, NETIF_F_TSO_MANGLEID_BIT);
    _netif_f!(GSO_SCTP, NETIF_F_GSO_SCTP_BIT);
    _netif_f!(GSO_UDP_L4, NETIF_F_GSO_UDP_L4_BIT);
    _netif_f!(GSO_FRAGLIST, NETIF_F_GSO_FRAGLIST_BIT);
    _netif_f!(HW_CSUM, NETIF_F_HW_CSUM_BIT);
    _netif_f!(HIGHDMA, NETIF_F_HIGHDMA_BIT);
    _netif_f!(LLTX, NETIF_F_LLTX_BIT);
    _netif_f!(GSO_GRE, NETIF_F_GSO_GRE_BIT);
    _netif_f!(GSO_GRE_CSUM, NETIF_F_GSO_GRE_CSUM_BIT);
    _netif_f!(GSO_IPXIP4, NETIF_F_GSO_IPXIP4_BIT);
    _netif_f!(GSO_IPXIP6, NETIF_F_GSO_IPXIP6_BIT);
    _netif_f!(GSO_UDP_TUNNEL, NETIF_F_GSO_UDP_TUNNEL_BIT);
    _netif_f!(GSO_UDP_TUNNEL_CSUM, NETIF_F_GSO_UDP_TUNNEL_CSUM_BIT);

    _netif_f_sum!(ALL_TSO, TSO, TSO6, TSO_ECN, TSO_MANGLEID);
    _netif_f_sum!(GSO_SOFTWARE, ALL_TSO, GSO_SCTP, GSO_UDP_L4, GSO_FRAGLIST);
    _netif_f_sum!(
        GSO_ENCAP_ALL,
        GSO_GRE,
        GSO_GRE_CSUM,
        GSO_IPXIP4,
        GSO_IPXIP6,
        GSO_UDP_TUNNEL,
        GSO_UDP_TUNNEL_CSUM
    );
}

/// Iff flags
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum Flag {
    /// UP
    UP = bindings::net_device_flags_IFF_UP,
    /// BROADCAST
    BROADCAST = bindings::net_device_flags_IFF_BROADCAST,
    /// DEBUG
    DEBUG = bindings::net_device_flags_IFF_DEBUG,
    /// LOOPBACK
    LOOPBACK = bindings::net_device_flags_IFF_LOOPBACK,
    /// POINTOPOINT
    POINTOPOINT = bindings::net_device_flags_IFF_POINTOPOINT,
    /// NOTRAILERS
    NOTRAILERS = bindings::net_device_flags_IFF_NOTRAILERS,
    /// RUNNING
    RUNNING = bindings::net_device_flags_IFF_RUNNING,
    /// NOARP
    NOARP = bindings::net_device_flags_IFF_NOARP,
    /// PROMISC
    PROMISC = bindings::net_device_flags_IFF_PROMISC,
    /// ALLMULTI
    ALLMULTI = bindings::net_device_flags_IFF_ALLMULTI,
    /// MASTER
    MASTER = bindings::net_device_flags_IFF_MASTER,
    /// SLAVE
    SLAVE = bindings::net_device_flags_IFF_SLAVE,
    /// MULTICAST
    MULTICAST = bindings::net_device_flags_IFF_MULTICAST,
    /// PORTSEL
    PORTSEL = bindings::net_device_flags_IFF_PORTSEL,
    /// AUTOMEDIA
    AUTOMEDIA = bindings::net_device_flags_IFF_AUTOMEDIA,
    /// DYNAMIC
    DYNAMIC = bindings::net_device_flags_IFF_DYNAMIC,

    // #if __UAPI_DEF_IF_NET_DEVICE_FLAGS_LOWER_UP_DORMANT_ECHO // TODO: is this needed?
    /// LOWER
    LOWER = bindings::net_device_flags_IFF_LOWER_UP,
    /// DORMANT
    DORMANT = bindings::net_device_flags_IFF_DORMANT,
    /// ECHO
    ECHO = bindings::net_device_flags_IFF_ECHO,
}

/// Iff private flags
#[repr(i32)]
#[allow(non_camel_case_types)]
pub enum PrivFlag {
    /// 802.1Q VLAN device.
    IFF_802_1Q_VLAN = bindings::netdev_priv_flags_IFF_802_1Q_VLAN, /* TODO: find a good name without leading 8 */
    /// Ethernet bridging device.
    EBRIDGE = bindings::netdev_priv_flags_IFF_EBRIDGE,
    /// Bonding master or slave.
    BONDING = bindings::netdev_priv_flags_IFF_BONDING,
    /// ISATAP interface (RFC4214).
    ISATAP = bindings::netdev_priv_flags_IFF_ISATAP,
    /// WAN HDLC device.
    WAN_HDLC = bindings::netdev_priv_flags_IFF_WAN_HDLC,
    /// dev_hard_start_xmit() is allowed to release skb->dst
    XMIT_DST_RELEASE = bindings::netdev_priv_flags_IFF_XMIT_DST_RELEASE,
    /// Disallow bridging this ether dev.
    DONT_BRIDGE = bindings::netdev_priv_flags_IFF_DONT_BRIDGE,
    /// Disable netpoll at run-time.
    DISABLE_NETPOLL = bindings::netdev_priv_flags_IFF_DISABLE_NETPOLL,
    /// Device used as macvlan port.
    MACVLAN_PORT = bindings::netdev_priv_flags_IFF_MACVLAN_PORT,
    /// Device used as bridge port.
    BRIDGE_PORT = bindings::netdev_priv_flags_IFF_BRIDGE_PORT,
    /// Device used as Open vSwitch datapath port.
    OVS_DATAPATH = bindings::netdev_priv_flags_IFF_OVS_DATAPATH,
    /// The interface supports sharing skbs on transmit.
    TX_SKB_SHARING = bindings::netdev_priv_flags_IFF_TX_SKB_SHARING,
    /// Supports unicast filtering.
    UNICAST_FLT = bindings::netdev_priv_flags_IFF_UNICAST_FLT,
    /// Device used as team port.
    TEAM_PORT = bindings::netdev_priv_flags_IFF_TEAM_PORT,
    /// Device supports sending custom FCS.
    SUPP_NOFCS = bindings::netdev_priv_flags_IFF_SUPP_NOFCS,
    /// Device supports hardware address change when it's running.
    LIVE_ADDR_CHANGE = bindings::netdev_priv_flags_IFF_LIVE_ADDR_CHANGE,
    /// Macvlan device.
    MACVLAN = bindings::netdev_priv_flags_IFF_MACVLAN,
    /// IFF_XMIT_DST_RELEASE not taking into account underlying stacked devices.
    XMIT_DST_RELEASE_PERM = bindings::netdev_priv_flags_IFF_XMIT_DST_RELEASE_PERM,
    /// Device is an L3 master device.
    L3MDEV_MASTER = bindings::netdev_priv_flags_IFF_L3MDEV_MASTER,
    /// Device can run without qdisc attached.
    NO_QUEUE = bindings::netdev_priv_flags_IFF_NO_QUEUE,
    /// Device is a Open vSwitch master.
    OPENVSWITCH = bindings::netdev_priv_flags_IFF_OPENVSWITCH,
    /// Device is enslaved to an L3 master device.
    L3MDEV_SLAVE = bindings::netdev_priv_flags_IFF_L3MDEV_SLAVE,
    /// Device is a team device.
    TEAM = bindings::netdev_priv_flags_IFF_TEAM,
    /// Device has had Rx Flow indirection table configured.
    RXFH_CONFIGURED = bindings::netdev_priv_flags_IFF_RXFH_CONFIGURED,
    /// The headroom value is controlled by an external entity (i.e. the master device for bridged veth).
    PHONY_HEADROOM = bindings::netdev_priv_flags_IFF_PHONY_HEADROOM,
    /// Device is a MACsec device.
    MACSEC = bindings::netdev_priv_flags_IFF_MACSEC,
    /// Device doesn't support the rx_handler hook.
    NO_RX_HANDLER = bindings::netdev_priv_flags_IFF_NO_RX_HANDLER,
    /// Device is a failover master device.
    FAILOVER = bindings::netdev_priv_flags_IFF_FAILOVER,
    /// Device is lower dev of a failover master device.
    FAILOVER_SLAVE = bindings::netdev_priv_flags_IFF_FAILOVER_SLAVE,
    /// Only invoke the rx handler of L3 master device.
    L3MDEV_RX_HANDLER = bindings::netdev_priv_flags_IFF_L3MDEV_RX_HANDLER,
    /// Rename is allowed while device is up and running.
    LIVE_RENAME_OK = bindings::netdev_priv_flags_IFF_LIVE_RENAME_OK,
}
