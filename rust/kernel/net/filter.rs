// SPDX-License-Identifier: GPL-2.0

//! Networking filters.
//!
//! C header: [`include/linux/netfilter.h`](../../../../../include/linux/netfilter.h)

use crate::{
    bindings,
    error::{code::*, to_result},
    net,
    types::PointerWrapper,
    ARef, AlwaysRefCounted, Result, ScopeGuard,
};
use alloc::boxed::Box;
use core::{
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
};

/// A network filter.
pub trait Filter {
    /// The type of the context data stored on registration and made available to the
    /// [`Filter::filter`] function.
    type Data: PointerWrapper + Sync = ();

    /// Filters the packet stored in the given buffer.
    ///
    /// It dictates to the netfilter core what the fate of the packet should be.
    fn filter(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _skb: &net::SkBuff,
    ) -> Disposition;
}

/// Specifies the action to be taken by the netfilter core.
pub enum Disposition {
    /// Drop the packet.
    Drop,

    /// Accept the packet.
    Accept,

    /// The packet was stolen by the filter and must be treated as if it didn't exist.
    Stolen,

    /// Queue the packet to the given user-space queue.
    Queue {
        /// The identifier of the queue to which the packet should be added.
        queue_id: u16,

        /// Specifies the behaviour if a queue with the given identifier doesn't exist: if `true`,
        /// the packet is accepted, otherwise it is rejected.
        accept_if_queue_non_existent: bool,
    },
}

/// The filter hook families.
pub enum Family {
    ///  IPv4 and IPv6 packets.
    INet(inet::Hook),

    /// IPv4 packets.
    Ipv4(ipv4::Hook, ipv4::PriorityBase),

    /// All packets through a device.
    ///
    /// When this family is used, a device _must_ be specified.
    NetDev(netdev::Hook),

    /// IPv6 packets.
    Ipv6(ipv6::Hook, ipv6::PriorityBase),

    /// Address resolution protocol (ARP) packets.
    Arp(arp::Hook),
}

/// A registration of a networking filter.
///
/// # Examples
///
/// The following is an example of a function that attaches an inbound filter (that always accepts
/// all packets after printing their lengths) on the specified device (in the `init` ns).
///
/// ```
/// use kernel::net::{self, filter as netfilter};
///
/// struct MyFilter;
/// impl netfilter::Filter for MyFilter {
///     fn filter(_data: (), skb: &net::SkBuff) -> netfilter::Disposition {
///         pr_info!("Packet of length {}\n", skb.len());
///         netfilter::Disposition::Accept
///     }
/// }
///
/// fn register(name: &CStr) -> Result<Pin<Box<netfilter::Registration<MyFilter>>>> {
///     let ns = net::init_ns();
///     let dev = ns.dev_get_by_name(name).ok_or(ENOENT)?;
///     netfilter::Registration::new_pinned(
///         netfilter::Family::NetDev(netfilter::netdev::Hook::Ingress),
///         0,
///         ns.into(),
///         Some(dev),
///         (),
///     )
/// }
/// ```
#[derive(Default)]
pub struct Registration<T: Filter> {
    hook: bindings::nf_hook_ops,
    // When `ns` is `Some(_)`, the hook is registered.
    ns: Option<ARef<net::Namespace>>,
    dev: Option<ARef<net::Device>>,
    _p: PhantomData<T>,
    _pinned: PhantomPinned,
}

// SAFETY: `Registration` does not expose any of its state across threads.
unsafe impl<T: Filter> Sync for Registration<T> {}

impl<T: Filter> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new() -> Self {
        Self {
            hook: bindings::nf_hook_ops::default(),
            dev: None,
            ns: None,
            _p: PhantomData,
            _pinned: PhantomPinned,
        }
    }

    /// Creates a new filter registration and registers it.
    ///
    /// Returns a pinned heap-allocated representation of the registration.
    pub fn new_pinned(
        family: Family,
        priority: i32,
        ns: ARef<net::Namespace>,
        dev: Option<ARef<net::Device>>,
        data: T::Data,
    ) -> Result<Pin<Box<Self>>> {
        let mut filter = Pin::from(Box::try_new(Self::new())?);
        filter.as_mut().register(family, priority, ns, dev, data)?;
        Ok(filter)
    }

    /// Registers a network filter.
    ///
    /// It must be pinned because the C portion of the kernel stores a pointer to it while it is
    /// registered.
    ///
    /// The priority is relative to the family's base priority. For example, if the base priority
    /// is `100` and `priority` is `-1`, the actual priority will be `99`. If a family doesn't
    /// explicitly allow a base to be specified, `0` is assumed.
    pub fn register(
        self: Pin<&mut Self>,
        family: Family,
        priority: i32,
        ns: ARef<net::Namespace>,
        dev: Option<ARef<net::Device>>,
        data: T::Data,
    ) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.ns.is_some() {
            // Already registered.
            return Err(EINVAL);
        }

        let data_pointer = data.into_pointer();

        // SAFETY: `data_pointer` comes from the call to `data.into_pointer()` above.
        let guard = ScopeGuard::new(|| unsafe {
            T::Data::from_pointer(data_pointer);
        });

        let mut pri_base = 0i32;
        match family {
            Family::INet(hook) => {
                this.hook.pf = bindings::NFPROTO_INET as _;
                this.hook.hooknum = hook as _;
            }
            Family::Ipv4(hook, pbase) => {
                this.hook.pf = bindings::NFPROTO_IPV4 as _;
                this.hook.hooknum = hook as _;
                pri_base = pbase as _;
            }
            Family::Ipv6(hook, pbase) => {
                this.hook.pf = bindings::NFPROTO_IPV6 as _;
                this.hook.hooknum = hook as _;
                pri_base = pbase as _;
            }
            Family::NetDev(hook) => {
                this.hook.pf = bindings::NFPROTO_NETDEV as _;
                this.hook.hooknum = hook as _;
            }
            Family::Arp(hook) => {
                this.hook.pf = bindings::NFPROTO_ARP as _;
                this.hook.hooknum = hook as _;
            }
        }

        this.hook.priority = pri_base.saturating_add(priority);
        this.hook.priv_ = data_pointer as _;
        this.hook.hook = Some(Self::hook_callback);
        crate::static_assert!(bindings::nf_hook_ops_type_NF_HOOK_OP_UNDEFINED == 0);

        if let Some(ref device) = dev {
            this.hook.dev = device.0.get();
        }

        // SAFETY: `ns` has a valid reference to the namespace, and `this.hook` was just
        // initialised above, so they're both valid.
        to_result(unsafe { bindings::nf_register_net_hook(ns.0.get(), &this.hook) })?;

        this.dev = dev;
        this.ns = Some(ns);
        guard.dismiss();
        Ok(())
    }

    unsafe extern "C" fn hook_callback(
        priv_: *mut core::ffi::c_void,
        skb: *mut bindings::sk_buff,
        _state: *const bindings::nf_hook_state,
    ) -> core::ffi::c_uint {
        // SAFETY: `priv_` was initialised on registration by a value returned from
        // `T::Data::into_pointer`, and it remains valid until the hook is unregistered.
        let data = unsafe { T::Data::borrow(priv_) };

        // SAFETY: The C contract guarantees that `skb` remains valid for the duration of this
        // function call.
        match T::filter(data, unsafe { net::SkBuff::from_ptr(skb) }) {
            Disposition::Drop => bindings::NF_DROP,
            Disposition::Accept => bindings::NF_ACCEPT,
            Disposition::Stolen => {
                // SAFETY: This function takes over ownership of `skb` when it returns `NF_STOLEN`,
                // so we decrement the refcount here to avoid a leak.
                unsafe { net::SkBuff::dec_ref(core::ptr::NonNull::new(skb).unwrap().cast()) };
                bindings::NF_STOLEN
            }
            Disposition::Queue {
                queue_id,
                accept_if_queue_non_existent,
            } => {
                // SAFETY: Just an FFI call, no additional safety requirements.
                let verdict = unsafe { bindings::NF_QUEUE_NR(queue_id as _) };
                if accept_if_queue_non_existent {
                    verdict | bindings::NF_VERDICT_FLAG_QUEUE_BYPASS
                } else {
                    verdict
                }
            }
        }
    }
}

impl<T: Filter> Drop for Registration<T> {
    fn drop(&mut self) {
        if let Some(ref ns) = self.ns {
            // SAFETY: `self.ns` is `Some(_)` only when a previous call to `nf_register_net_hook`
            // succeeded. And the arguments are the same.
            unsafe { bindings::nf_unregister_net_hook(ns.0.get(), &self.hook) };

            // `self.hook.priv_` was initialised during registration to a value returned from
            // `T::Data::into_pointer`, so it is ok to convert back here.
            unsafe { T::Data::from_pointer(self.hook.priv_) };
        }
    }
}

/// Definitions used when defining hooks for the [`Family::NetDev`] family.
pub mod netdev {
    use crate::bindings;

    /// Hooks allowed in the [`super::Family::NetDev`] family.
    #[repr(u32)]
    pub enum Hook {
        /// All inbound packets through the given device.
        Ingress = bindings::nf_dev_hooks_NF_NETDEV_INGRESS,

        /// All outbound packets through the given device.
        Egress = bindings::nf_dev_hooks_NF_NETDEV_EGRESS,
    }
}

/// Definitions used when defining hooks for the [`Family::Ipv4`] family.
pub mod ipv4 {
    use crate::bindings;

    /// Hooks allowed in [`super::Family::Ipv4`] family.
    pub type Hook = super::inet::Hook;

    /// The base priority for [`super::Family::Ipv4`] hooks.
    ///
    /// The actual priority is the base priority plus the priority specified when registering.
    #[repr(i32)]
    pub enum PriorityBase {
        /// Same as the `NF_IP_PRI_FIRST` C constant.
        First = bindings::nf_ip_hook_priorities_NF_IP_PRI_FIRST,

        /// Same as the `NF_IP_PRI_RAW_BEFORE_DEFRAG` C constant.
        RawBeforeDefrag = bindings::nf_ip_hook_priorities_NF_IP_PRI_RAW_BEFORE_DEFRAG,

        /// Same as the `NF_IP_PRI_CONNTRACK_DEFRAG` C constant.
        ConnTrackDefrag = bindings::nf_ip_hook_priorities_NF_IP_PRI_CONNTRACK_DEFRAG,

        /// Same as the `NF_IP_PRI_RAW` C constant.
        Raw = bindings::nf_ip_hook_priorities_NF_IP_PRI_RAW,

        /// Same as the `NF_IP_PRI_SELINUX_FIRST` C constant.
        SeLinuxFirst = bindings::nf_ip_hook_priorities_NF_IP_PRI_SELINUX_FIRST,

        /// Same as the `NF_IP_PRI_CONNTRACK` C constant.
        ConnTrack = bindings::nf_ip_hook_priorities_NF_IP_PRI_CONNTRACK,

        /// Same as the `NF_IP_PRI_MANGLE` C constant.
        Mangle = bindings::nf_ip_hook_priorities_NF_IP_PRI_MANGLE,

        /// Same as the `NF_IP_PRI_NAT_DST` C constant.
        NatDst = bindings::nf_ip_hook_priorities_NF_IP_PRI_NAT_DST,

        /// Same as the `NF_IP_PRI_FILTER` C constant.
        Filter = bindings::nf_ip_hook_priorities_NF_IP_PRI_FILTER,

        /// Same as the `NF_IP_PRI_SECURITY` C constant.
        Security = bindings::nf_ip_hook_priorities_NF_IP_PRI_SECURITY,

        /// Same as the `NF_IP_PRI_NAT_SRC` C constant.
        NatSrc = bindings::nf_ip_hook_priorities_NF_IP_PRI_NAT_SRC,

        /// Same as the `NF_IP_PRI_SELINUX_LAST` C constant.
        SeLinuxLast = bindings::nf_ip_hook_priorities_NF_IP_PRI_SELINUX_LAST,

        /// Same as the `NF_IP_PRI_CONNTRACK_HELPER` C constant.
        ConnTrackHelper = bindings::nf_ip_hook_priorities_NF_IP_PRI_CONNTRACK_HELPER,

        /// Same as the `NF_IP_PRI_LAST` and `NF_IP_PRI_CONNTRACK_CONFIRM` C constants.
        Last = bindings::nf_ip_hook_priorities_NF_IP_PRI_LAST,
    }
}

/// Definitions used when defining hooks for the [`Family::Ipv6`] family.
pub mod ipv6 {
    use crate::bindings;

    /// Hooks allowed in [`super::Family::Ipv6`] family.
    pub type Hook = super::inet::Hook;

    /// The base priority for [`super::Family::Ipv6`] hooks.
    ///
    /// The actual priority is the base priority plus the priority specified when registering.
    #[repr(i32)]
    pub enum PriorityBase {
        /// Same as the `NF_IP6_PRI_FIRST` C constant.
        First = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_FIRST,

        /// Same as the `NF_IP6_PRI_RAW_BEFORE_DEFRAG` C constant.
        RawBeforeDefrag = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_RAW_BEFORE_DEFRAG,

        /// Same as the `NF_IP6_PRI_CONNTRACK_DEFRAG` C constant.
        ConnTrackDefrag = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_CONNTRACK_DEFRAG,

        /// Same as the `NF_IP6_PRI_RAW` C constant.
        Raw = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_RAW,

        /// Same as the `NF_IP6_PRI_SELINUX_FIRST` C constant.
        SeLinuxFirst = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_SELINUX_FIRST,

        /// Same as the `NF_IP6_PRI_CONNTRACK` C constant.
        ConnTrack = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_CONNTRACK,

        /// Same as the `NF_IP6_PRI_MANGLE` C constant.
        Mangle = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_MANGLE,

        /// Same as the `NF_IP6_PRI_NAT_DST` C constant.
        NatDst = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_NAT_DST,

        /// Same as the `NF_IP6_PRI_FILTER` C constant.
        Filter = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_FILTER,

        /// Same as the `NF_IP6_PRI_SECURITY` C constant.
        Security = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_SECURITY,

        /// Same as the `NF_IP6_PRI_NAT_SRC` C constant.
        NatSrc = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_NAT_SRC,

        /// Same as the `NF_IP6_PRI_SELINUX_LAST` C constant.
        SeLinuxLast = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_SELINUX_LAST,

        /// Same as the `NF_IP6_PRI_CONNTRACK_HELPER` C constant.
        ConnTrackHelper = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_CONNTRACK_HELPER,

        /// Same as the `NF_IP6_PRI_LAST` C constant.
        Last = bindings::nf_ip6_hook_priorities_NF_IP6_PRI_LAST,
    }
}

/// Definitions used when defining hooks for the [`Family::Arp`] family.
pub mod arp {
    use crate::bindings;

    /// Hooks allowed in the [`super::Family::Arp`] family.
    #[repr(u32)]
    pub enum Hook {
        /// Inbound ARP packets.
        In = bindings::NF_ARP_IN,

        /// Outbound ARP packets.
        Out = bindings::NF_ARP_OUT,

        /// Forwarded ARP packets.
        Forward = bindings::NF_ARP_FORWARD,
    }
}

/// Definitions used when defining hooks for the [`Family::INet`] family.
pub mod inet {
    use crate::bindings;

    /// Hooks allowed in the [`super::Family::INet`], [`super::Family::Ipv4`], and
    /// [`super::Family::Ipv6`] families.
    #[repr(u32)]
    pub enum Hook {
        /// Inbound packets before routing decisions are made (i.e., before it's determined if the
        /// packet is to be delivered locally or forwarded to another host).
        PreRouting = bindings::nf_inet_hooks_NF_INET_PRE_ROUTING as _,

        /// Inbound packets that are meant to be delivered locally.
        LocalIn = bindings::nf_inet_hooks_NF_INET_LOCAL_IN as _,

        /// Inbound packets that are meant to be forwarded to another host.
        Forward = bindings::nf_inet_hooks_NF_INET_FORWARD as _,

        /// Outbound packet created by the local networking stack.
        LocalOut = bindings::nf_inet_hooks_NF_INET_LOCAL_OUT as _,

        /// All outbound packets (i.e., generated locally or being forwarded to another host).
        PostRouting = bindings::nf_inet_hooks_NF_INET_POST_ROUTING as _,

        /// Equivalent to [`super::netdev::Hook::Ingress`], so a device must be specified. Packets
        /// of all types (not just ipv4/ipv6) will be delivered to the filter.
        Ingress = bindings::nf_inet_hooks_NF_INET_INGRESS as _,
    }
}
