// SPDX-License-Identifier: GPL-2.0

//! Net Device Operations.
//!
//! C header: [`include/linux/netdevice.h`](../../../../include/linux/netdevice.h)

use core::{marker, mem, ptr};

use crate::bindings;
use crate::from_kernel_result;
use crate::error::{Error, Result};
use crate::types::{SavedAsPointer, SavedAsPointerMut};
use crate::{c_types, str::CStr};

use super::ethtool::EthToolOps;
use super::rtnl::{RtnlLinkStats64, RtnlLock};
use super::skbuff::SkBuff;

extern "C" {
    #[allow(improper_ctypes)]
    fn rust_helper_netdev_priv(dev: *const bindings::net_device) -> *mut c_types::c_void;

    #[allow(improper_ctypes)]
    fn rust_helper_eth_hw_addr_random(dev: *const bindings::net_device);

    #[allow(improper_ctypes)]
    fn rust_helper_net_device_set_new_lstats(dev: *mut bindings::net_device) -> c_types::c_int;

    #[allow(improper_ctypes)]
    fn rust_helper_dev_lstats_add(dev: *mut bindings::net_device, len: u32);
}

/// interface name assignment types (sysfs name_assign_type attribute).
#[repr(u8)]
pub enum NetNameAssingType {
    /// Unknown network name assing type.
    Unknown = bindings::NET_NAME_UNKNOWN as u8,
    /// Enum network name assing type.
    Enum = bindings::NET_NAME_ENUM as u8,
}

unsafe extern "C" fn setup_netdev_callback<T: NetDeviceAdapter>(dev: *mut bindings::net_device) {
    let mut dev = unsafe { NetDevice::<T>::from_pointer(dev) };

    T::setup(&mut dev);
}

/// Wraps the kernel's `struct net_device`.
///
/// # Invariants
///
/// The pointer [`NetDevice::ptr`] is non-null and valid.
#[repr(transparent)]
pub struct NetDevice<T: NetDeviceAdapter> {
    ptr: *const bindings::net_device,
    priv_data: marker::PhantomData<T>,
}

unsafe impl<T: NetDeviceAdapter> Sync for NetDevice<T> {}

impl<T: NetDeviceAdapter> NetDevice<T> {
    /// Allocate and create a new NetDevice with private data T.
    /// This function locks [`RtnlLock`].
    pub fn new(
        priv_data: T,
        format_name: &CStr,
        name_assign_type: NetNameAssingType,
        txqs: u32,
        rxqs: u32,
    ) -> Result<Self> {
        let _lock = RtnlLock::lock();
        // SAFETY: Lock is hold.
        let dev = unsafe { Self::new_locked(priv_data, format_name, name_assign_type, txqs, rxqs) };
        dev
    }

    /// Allocate and create a new NetDevice with private data T.
    /// No lock is accuired by this function, therefor this function is unsafe.
    ///
    /// # Safety
    ///
    /// The caller has to hold the [`RtnlLock`].
    pub unsafe fn new_locked(
        priv_data: T,
        format_name: &CStr,
        name_assign_type: NetNameAssingType,
        txqs: u32,
        rxqs: u32,
    ) -> Result<Self> {
        if txqs < 1 || rxqs < 1 {
            return Err(Error::EINVAL);
        }
        let size = mem::size_of::<T>() as i32;

        let ptr = unsafe {
            bindings::alloc_netdev_mqs(
                size,
                format_name.as_ptr() as _,
                name_assign_type as u8,
                Some(setup_netdev_callback::<T>),
                txqs,
                rxqs,
           )
        };
        if ptr.is_null() {
            return Err(Error::ENOMEM);
        }

        if size != 0 {
            unsafe {
                let dest = rust_helper_netdev_priv(ptr) as *mut T;
                ptr::write(dest, priv_data);
            }
        }

        Ok(Self {
            ptr,
            priv_data: marker::PhantomData::<T>,
        })
    }

    /// Return a refference to the private data of the [`NetDevice`].
    pub fn get_priv_data(&self) -> &T {
        // SAFETY: self.ptr is valid if self is valid
        let priv_ptr = unsafe { rust_helper_netdev_priv(self.ptr) } as *mut T;

        // SAFETY: ptr is valid and of type T
        unsafe { priv_ptr.as_ref() }.unwrap()
    }

    /// Return a mutable refference of the private data of the [`NetDevice`]
    pub fn get_priv_data_mut(&mut self) -> &mut T {
        // SAFETY: self.ptr is valid if self is valid
        let priv_ptr = unsafe { rust_helper_netdev_priv(self.ptr) as *mut T };

        // SAFETY: ptr is valid and of type T
        unsafe { priv_ptr.as_mut().unwrap() }
    }

    /// Setup Ethernet network device.
    ///
    /// Fill in the fields of the device structure with Ethernet-generic values.
    pub fn ether_setup(&mut self) {
        // SAFETY: self.ptr is valid
        unsafe { bindings::ether_setup(self.ptr as *mut bindings::net_device) }
    }

    /// Generate software assigned random Ethernet and set device flag.
    ///
    /// Generate a random Ethernet address (MAC) to be used by a net device
    /// and set addr_assign_type so the state can be read by sysfs and be
    /// used by userspace.
    pub fn hw_addr_random(&mut self) {
        // SAFETY: self.ptr is valid
        unsafe { rust_helper_eth_hw_addr_random(self.ptr) };
    }

    /// Register a network device.
    ///
    /// Take a completed network device structure and add it to the kernel
    /// interfaces. A %NETDEV_REGISTER message is sent to the netdev notifier
    /// chain.
    ///
    /// This is a wrapper around register_netdevice that takes the rtnl semaphore
    /// and expands the device name if you passed a format string to
    /// alloc_netdev.
    pub fn register(&self) -> Result {
        // SAFETY: self.ptr is valid
        let err = unsafe { bindings::register_netdev(self.ptr as *mut bindings::net_device) };

        if err != 0 {
            Err(Error::from_kernel_errno(err))
        } else {
            Ok(())
        }
    }

    /// Register a network device if the RtnlLock is already hold.
    ///
    /// Take a completed network device structure and add it to the kernel
    /// interfaces. A %NETDEV_REGISTER message is sent to the netdev notifier
    /// chain. 0 is returned on success. A negative errno code is returned
    /// on a failure to set up the device, or if the name is a duplicate.
    ///
    /// Callers must hold the rtnl semaphore. You may want
    /// [`register`] instead of this.
    ///
    /// BUGS:
    /// The locking appears insufficient to guarantee two parallel registers
    /// will not get the same name.
    ///
    /// # Safety
    ///
    /// caller must hold the RtnlLock and semaphore
    pub unsafe fn register_locked(&self) -> Result {
        let err = unsafe { bindings::register_netdevice(self.ptr as *mut bindings::net_device) };

        if err != 0 {
            Err(Error::from_kernel_errno(err))
        } else {
            Ok(())
        }
    }

    /// Set the rtnl_link_ops to a network interface.
    ///
    /// Takes a static mut created with [`crate::net::rtnl_link_ops!`] and assing it to [`self`].
    pub fn set_rtnl_ops(&self, ops: &'static super::rtnl::RtnlLinkOps) {
        // get rtnl_lock
        let _lock = RtnlLock::lock();

        // SAFETY: lock is hold
        unsafe { self.set_rtnl_ops_locked(ops) }
    }

    /// Set the rtnl_link_ops to a network interface, while the caller holds the rtnl_lock.
    ///
    /// Takes a static mut created with [`crate::net::rtnl_link_ops!`] and assing it to self.
    ///
    /// # Safety
    ///
    /// The caller has to hold the [`RtnlLock`].
    pub unsafe fn set_rtnl_ops_locked(&self, ops: &'static super::rtnl::RtnlLinkOps) {
        // SAFETY: prt is valid if self is valid
        unsafe { 
            let mut dev = (self.ptr as *mut bindings::net_device).as_mut().unwrap();

            dev.rtnl_link_ops = ops.get_pointer() as *mut bindings::rtnl_link_ops;
        }
    }

    /// Add a [`Iff`] flag to the [`NetDevice`].
    pub fn add_flag(&mut self, flag: Iff) {
        // SAFETY: prt is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut().unwrap() };

        dev.flags |= flag as u32;
    }

    /// Remove a [`Iff`] flag from the [`NetDevice`].
    pub fn remove_flag(&mut self, flag: Iff) {
        // SAFETY: prt is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut().unwrap() };

        dev.flags &= !(flag as u32);
    }

    /// Add a [`IffPriv`] private_flag to the [`NetDevice`].
    pub fn add_private_flag(&mut self, flag: IffPriv) {
        // SAFETY: prt is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut().unwrap() };

        dev.priv_flags |= flag as u32;
    }

    /// Remove a [`IffPriv`] private_flag from the [`NetDevice`].
    pub fn remove_private_flag(&mut self, flag: IffPriv) {
        // SAFETY: prt is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut().unwrap() };

        dev.priv_flags &= !(flag as u32);
    }

    /// Set a [`feature::NetIF`] `feature` set to the [`NetDevice`].
    pub fn set_features(&mut self, features: feature::NetIF) {
        // SAFETY: ptr is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut() }.unwrap();

        dev.features = features.into();
    }

    /// Get the [`feature::NetIF`] `feature` set from the [`NetDevice`].
    pub fn get_features(&self) -> feature::NetIF {
        // SAFETY: ptr is valid if self is valid
        let dev = unsafe { self.ptr.as_ref() }.unwrap();

        feature::NetIF::from(dev.features)
    }

    /// Set a [`feature::NetIF`] `hw_feature` set to the [`NetDevice`].
    pub fn set_hw_features(&mut self, features: feature::NetIF) {
        // SAFETY: ptr is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut() }.unwrap();

        dev.hw_features = features.into();
    }

    /// Get the [`feature::NetIF`] `hw_feature` set from the [`NetDevice`].
    pub fn get_hw_features(&self) -> feature::NetIF {
        // SAFETY: ptr is valid if self is valid
        let dev = unsafe { self.ptr.as_ref() }.unwrap();

        feature::NetIF::from(dev.hw_features)
    }

    /// Set a [`feature::NetIF`] `hw_enc_feature` set to the [`NetDevice`].
    pub fn set_hw_enc_features(&mut self, features: feature::NetIF) {
        // SAFETY: ptr is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut() }.unwrap();

        dev.hw_enc_features = features.into();
    }

    /// Get the [`feature::NetIF`] `hw_enc_feature` set from the [`NetDevice`].
    pub fn get_hw_enc_features(&self) -> feature::NetIF {
        // SAFETY: ptr is valid if self is valid
        let dev = unsafe { self.ptr.as_ref() }.unwrap();

        feature::NetIF::from(dev.hw_enc_features)
    }

    /// Set mut for the [`NetDevice`].
    pub fn set_mtu(&mut self, min: u32, max: u32) {
        // SAFETY: prt is valid if self is valid
        let mut dev = unsafe { (self.ptr as *mut bindings::net_device).as_mut().unwrap() };

        dev.min_mtu = min;
        dev.max_mtu = max;
    }

    /// Create a new `pcpu_lstats` struct and assing it to the [`NetDevice`].
    // This is more or less a workaround, as I did not find a way to create a pcpu marco
    // and assing some value to the anonymous union.
    pub fn set_new_pcpu_lstats(&mut self) -> Result {
        // SAFETY: calling c function
        let ret =
            unsafe { rust_helper_net_device_set_new_lstats(self.ptr as *mut bindings::net_device) };

        if ret != 0 {
            Err(Error::from_kernel_errno(ret))
        } else {
            Ok(())
        }
    }

    /// Free the lstats field.
    /// # Safety
    ///
    /// Only call when the same device had set_new_pcpu_lstats called
    pub unsafe fn free_lstats(&mut self) {
        // SAFETY: self.ptr->lstats is valid if self is valid
        let net_device: &bindings::net_device = self.get_internal();

        unsafe {
            let lstats = net_device.__bindgen_anon_1.lstats;
            if !lstats.is_null() {
                bindings::free_percpu(lstats as *mut _)
            }
        }
    }

    /// Add a value the the internal lstats.
    pub fn lstats_add(&mut self, len: u32) {
        // SAFETD: calling c function
        unsafe {
            rust_helper_dev_lstats_add(self.ptr as *mut bindings::net_device, len);
        }
    }

    /// Set carrier.
    pub fn carrier_set(&mut self, status: bool) {
        // SAFETY: self.ptr is valid if self is valid
        if status {
            unsafe { bindings::netif_carrier_on(self.ptr as *mut bindings::net_device) }
        } else {
            unsafe { bindings::netif_carrier_off(self.ptr as *mut bindings::net_device) }
        }
    }

    /// Set `netdev_ops` and `ethtool_ops` from the [`NetDeviceAdapter`] T to the [`NetDevice`].
    /// This also sets `needs_free_netdev` to true.
    pub fn set_ops(&mut self) {
        let internal = self.get_internal_mut();
        internal.needs_free_netdev = true;
        // SAFETY: T is valid for this netdevice, so build is valid.
        unsafe {
            internal.netdev_ops = NetDeviceOperationsVtable::<T>::build();
            internal.ethtool_ops = super::ethtool::EthToolOperationsVtable::<T>::build();
        }
    }
}

impl<I: NetDeviceAdapter> SavedAsPointer for NetDevice<I> {
    type InternalType = bindings::net_device;

    unsafe fn from_pointer(ptr: *const Self::InternalType) -> Self {
        Self {
            ptr,
            priv_data: marker::PhantomData::<I>,
        }
    }

    fn get_pointer(&self) -> *const Self::InternalType {
        self.ptr
    }
}

impl<I: NetDeviceAdapter> SavedAsPointerMut for NetDevice<I> {}

/// Trait holding the type of the NetDevice, and implementing the setup function.
pub trait NetDeviceAdapter: Sized {
    /// Type of the Inner Private data field
    type Inner: Sized; // = Self

    /// Type ipmlementing all functions used for [`NetDeviceOps`].
    type Ops: NetDeviceOps<Self>;

    /// Type implementing all functions used for [`EthToolOps`].
    type EthOps: EthToolOps<Self>;

    /// Callback to initialize the device
    /// Function tables have to be assinged via [`NetDevice::set_ops`]
    fn setup(dev: &mut NetDevice<Self>);
}

#[repr(i32)]
#[allow(non_camel_case_types)]
/// Maps to [`bindings::netdev_tx`] from the kernel.
pub enum NetdevTX {
    /// TX_OK
    TX_OK = bindings::netdev_tx_NETDEV_TX_OK,
    /// TX_BUSY
    TX_BUSY = bindings::netdev_tx_NETDEV_TX_BUSY,
}

unsafe extern "C" fn ndo_init_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
) -> c_types::c_int {
    from_kernel_result! {
        T::Ops::init(
            unsafe { &mut NetDevice::<T>::from_pointer(dev) }
        )?;
        Ok(0)
    }
}

unsafe extern "C" fn ndo_uninit_callback<T: NetDeviceAdapter>(dev: *mut bindings::net_device) {
    // SAFETY: pointer is valid as it comes form C
    T::Ops::uninit(
        unsafe { &mut NetDevice::<T>::from_pointer(dev)}
    );
}

unsafe extern "C" fn ndo_start_xmit_callback<T: NetDeviceAdapter>(
    skb: *mut bindings::sk_buff,
    dev: *mut bindings::net_device,
) -> bindings::netdev_tx_t {
    T::Ops::start_xmit(
        unsafe { SkBuff::from_pointer(skb) },
        unsafe { &mut NetDevice::from_pointer(dev)}
    ) as bindings::netdev_tx_t
}

unsafe extern "C" fn ndo_get_stats64_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
    stats: *mut bindings::rtnl_link_stats64,
) {
    T::Ops::get_stats64(
        unsafe { &NetDevice::<T>::from_pointer(dev) },
        unsafe { &mut RtnlLinkStats64::from_pointer(stats) },
    );
}

unsafe extern "C" fn ndo_change_carrier_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
    change_carrier: bool,
) -> c_types::c_int {
    from_kernel_result! {
        T::Ops::change_carrier(
            unsafe { &mut NetDevice::<T>::from_pointer(dev) },
            change_carrier
        )?;
        Ok(0)
    }
}

unsafe extern "C" fn ndo_validate_addr_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
) -> c_types::c_int {
    from_kernel_result! {
        T::Ops::validate_addr(
            unsafe { &NetDevice::<T>::from_pointer(dev) })?;
        Ok(0)
    }
}

unsafe extern "C" fn ndo_set_mac_address_callback<T: NetDeviceAdapter>(
    dev: *mut bindings::net_device,
    p: *mut c_types::c_void,
) -> c_types::c_int {
    from_kernel_result! {
        T::Ops::set_mac_addr(
            unsafe { &mut NetDevice::<T>::from_pointer(dev) },
            p
        )?;
        Ok(0)
    }
}

unsafe extern "C" fn ndo_set_rx_mode_callback<T: NetDeviceAdapter>(dev: *mut bindings::net_device) {
    T::Ops::set_rx_mode(
        unsafe { &mut NetDevice::<T>::from_pointer(dev) }
    )
}

pub(crate) struct NetDeviceOperationsVtable<T: NetDeviceAdapter>(marker::PhantomData<T>);

impl<T: NetDeviceAdapter> NetDeviceOperationsVtable<T> {
    const VTABLE: bindings::net_device_ops = bindings::net_device_ops {
        ndo_init: Some(ndo_init_callback::<T>),
        ndo_uninit: Some(ndo_uninit_callback::<T>),
        ndo_open: None,
        ndo_stop: None,
        ndo_start_xmit: Some(ndo_start_xmit_callback::<T>),
        ndo_features_check: None,
        ndo_select_queue: None,
        ndo_change_rx_flags: None,
        ndo_set_rx_mode: if T::Ops::TO_USE.set_rx_mode {
            Some(ndo_set_rx_mode_callback::<T>)
        } else {
            None
        },
        ndo_set_mac_address: if T::Ops::TO_USE.set_mac_addr {
            Some(ndo_set_mac_address_callback::<T>)
        } else {
            None
        },
        ndo_validate_addr: if T::Ops::TO_USE.validate_addr {
            Some(ndo_validate_addr_callback::<T>)
        } else {
            None
        },
        ndo_do_ioctl: None,
        ndo_set_config: None,
        ndo_change_mtu: None,
        ndo_neigh_setup: None,
        ndo_tx_timeout: None,
        ndo_get_stats64: if T::Ops::TO_USE.get_stats64 {
            Some(ndo_get_stats64_callback::<T>)
        } else {
            None
        },
        ndo_has_offload_stats: None,
        ndo_get_offload_stats: None,
        ndo_get_stats: None,
        ndo_vlan_rx_add_vid: None,
        ndo_vlan_rx_kill_vid: None,

        #[cfg(CONFIG_NET_POLL_CONTROLLER)]
        ndo_poll_controller: None,
        #[cfg(CONFIG_NET_POLL_CONTROLLER)]
        ndo_netpoll_setup: None,
        #[cfg(CONFIG_NET_POLL_CONTROLLER)]
        ndo_netpoll_cleanup: None,

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

        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_enable: None,
        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_disable: None,
        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_ddp_setup: None,
        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_ddp_done: None,
        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_ddp_target: None,
        #[cfg(any(CONFIG_FCOE = "y", CONFIG_FCOE = "m"))]
        ndo_fcoe_get_hbainfo: None,

        #[cfg(any(CONFIG_LIBFCOE = "y", CONFIG_LIBFCOE = "m"))]
        ndo_fcoe_get_wwn: None,

        #[cfg(CONFIG_RFS_ACCEL)]
        ndo_rx_flow_steer: None,

        ndo_add_slave: None,
        ndo_del_slave: None,
        ndo_get_xmit_slave: None,
        ndo_sk_get_lower_dev: None,
        ndo_fix_features: None,
        ndo_set_features: None,
        ndo_neigh_construct: None,
        ndo_neigh_destroy: None,
        ndo_fdb_add: None,
        ndo_fdb_del: None,
        ndo_fdb_dump: None,
        ndo_fdb_get: None,
        ndo_bridge_setlink: None,
        ndo_bridge_getlink: None,
        ndo_bridge_dellink: None,
        ndo_change_carrier: if T::Ops::TO_USE.change_carrier {
            Some(ndo_change_carrier_callback::<T>)
        } else {
            None
        },
        ndo_get_phys_port_id: None,
        ndo_get_port_parent_id: None,
        ndo_get_phys_port_name: None,
        ndo_dfwd_add_station: None,
        ndo_dfwd_del_station: None,
        ndo_set_tx_maxrate: None,
        ndo_get_iflink: None,
        ndo_change_proto_down: None,
        ndo_fill_metadata_dst: None,
        ndo_set_rx_headroom: None,
        ndo_bpf: None,
        ndo_xdp_xmit: None,
        ndo_xsk_wakeup: None,
        ndo_get_devlink_port: None,
        ndo_tunnel_ctl: None,
        ndo_get_peer_dev: None,
    };

    /// Builds an instance of [`struct net_device_ops`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) const unsafe fn build() -> &'static bindings::net_device_ops {
        &Self::VTABLE
    }
}

/// Represents which fields of [`struct net_device_ops`] should pe populated with pointers for the trait [`NetDeviceOps`].
pub struct ToUse {
    /// Trait defines a `ndo_change_carrier` function.
    pub change_carrier: bool,

    /// Trait defines a `ndo_get_stats64` function.
    pub get_stats64: bool,

    /// Trait defines a `ndo_validate_addr` function.
    pub validate_addr: bool,

    /// Trait defines a `ndo_set_mac_addr` function.
    pub set_mac_addr: bool,

    /// Trait defines a `ndo_set_rx_mode` function.
    pub set_rx_mode: bool,
}

/// This trait does not include any functions exept [`init`] and [`uninit`].
#[doc(hidden)]
pub const USE_NONE: ToUse = ToUse {
    change_carrier: false,
    get_stats64: false,
    validate_addr: false,
    set_mac_addr: false,
    set_rx_mode: false,
};

/// Defines the [`NetDeviceOps::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_net_device_ops {
    () => {
        const TO_USE: $crate::net::device::ToUse = $crate::net::device::USE_NONE;
    };
    ($($i:ident),+) => {
        const TO_USE: kernel::net::device::ToUse =
            $crate::net::device::ToUse {
                $($i: true),+ ,
                ..$crate::net::device::USE_NONE
            };
    };
}

/// Corresponds to the kernel's `struct net_device_ops`.
///
/// You Implement this trait whenever you would create a `struct net_device_ops`.
pub trait NetDeviceOps<T: NetDeviceAdapter>: Send + Sync + Sized {
    /// The methods to use to populate [`struct net_device_ops`].
    const TO_USE: ToUse;

    /// This function is called once when a network device is registered.
    /// The network device can use this for any late stage initialization
    /// or semantic validation. It can fail with an error code which will
    /// be propagated back to register_netdev.
    fn init(dev: &mut NetDevice<T>) -> Result;

    /// This function is called when device is unregistered or when registration
    /// fails. It is not called if init fails.
    fn uninit(dev: &mut NetDevice<T>);

    /// Called when a packet needs to be transmitted.
    /// `Ok(())` returns NETDEV_TX_OK, Error maps to `NETDEV_TX_BUSY`
    /// Returns NETDEV_TX_OK.  Can return NETDEV_TX_BUSY, but you should stop
    /// the queue before that can happen; it's for obsolete devices and weird
    /// corner cases, but the stack really does a non-trivial amount
    /// of useless work if you return NETDEV_TX_BUSY.
    fn start_xmit(_skb: SkBuff, _dev: &mut NetDevice<T>) -> NetdevTX {
        NetdevTX::TX_OK
    }

    /// Called when a user wants to get the network device usage
    /// statistics.
    ///
    /// Must fill in a zero-initialised [`RtnlLinkStats64`] structure
    /// passed by the caller.
    fn get_stats64(_dev: &NetDevice<T>, _stats: &mut RtnlLinkStats64) {}

    /// Called to change device carrier. Soft-devices (like dummy, team, etc)
    /// which do not represent real hardware may define this to allow their
    /// userspace components to manage their virtual carrier state. Devices
    /// that determine carrier state from physical hardware properties (eg
    /// network cables) or protocol-dependent mechanisms (eg
    /// USB_CDC_NOTIFY_NETWORK_CONNECTION) should NOT implement this function, and
    /// therefor NOT set [`TO_USE.change_carrier`].
    fn change_carrier(_dev: &mut NetDevice<T>, _new_carrier: bool) -> Result {
        Err(Error::EINVAL)
    }

    /// Test if Media Access Control address is valid for the device.
    fn validate_addr(_dev: &NetDevice<T>) -> Result {
        Err(Error::EINVAL)
    }

    /// This function  is called when the Media Access Control address
    /// needs to be changed. If this interface is not defined, the
    /// MAC address can not be changed.
    fn set_mac_addr(_dev: &mut NetDevice<T>, _p: *mut c_types::c_void) -> Result {
        Err(Error::EINVAL)
    }

    /// This function is called device changes address list filtering.
    /// If driver handles unicast address filtering, it should set
    /// IFF_UNICAST_FLT in its priv_flags.
    fn set_rx_mode(_dev: &mut NetDevice<T>) {}
}

/// Iff flags
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum Iff {
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
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum IffPriv {
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

/// Feature flags.
pub mod feature {
    use core::convert::{From, Into};
    use core::ops::{Add, AddAssign, Deref, DerefMut, Sub, SubAssign};

    /// Holds multiple flags to give to an interface via [`NetDevice::set_flag`].
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy)]
    pub struct NetIF(u64);

    impl NetIF {
        /// Create new Flag with value `0`.
        pub const fn new() -> Self {
            Self(0)
        }

        /// Add flag to Self.
        pub fn add_flag(&mut self, flag: u64) {
            self.0 |= flag;
        }

        /// Remove the given flag from Self.
        pub fn remove_flag(&mut self, flag: u64) {
            self.0 &= !(flag);
        }
    }

    impl Deref for NetIF {
        type Target = u64;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl DerefMut for NetIF {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    impl Add for NetIF {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self::from(self.0 | rhs.0)
        }
    }

    impl Add<u64> for NetIF {
        type Output = Self;

        fn add(self, rhs: u64) -> Self::Output {
            Self::from(self.0 | rhs)
        }
    }

    impl Sub for NetIF {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            Self::from(self.0 & !rhs.0)
        }
    }

    impl Sub<u64> for NetIF {
        type Output = Self;

        fn sub(self, rhs: u64) -> Self::Output {
            Self::from(self.0 & !rhs)
        }
    }

    impl AddAssign for NetIF {
        fn add_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0
        }
    }

    impl AddAssign<u64> for NetIF {
        fn add_assign(&mut self, rhs: u64) {
            self.0 |= rhs
        }
    }

    impl SubAssign for NetIF {
        fn sub_assign(&mut self, rhs: Self) {
            self.0 &= !rhs.0
        }
    }

    impl SubAssign<u64> for NetIF {
        fn sub_assign(&mut self, rhs: u64) {
            self.0 &= !rhs
        }
    }

    impl From<u64> for NetIF {
        fn from(flags: u64) -> Self {
            Self(flags)
        }
    }

    impl Into<u64> for NetIF {
        fn into(self) -> u64 {
            self.0
        }
    }

    macro_rules! _netif_f {
        ($name:ident, $binding:ident) => {
            /// [`NetIF`] flag for $binding
            pub const $name: u64 = 1u64 << $crate::bindings::$binding;
        };
    }

    macro_rules! _netif_f_sum {
        ($name:ident, $($f:ident),+) => {
            /// [`NetIF`] flag for $name
            pub const $name: u64 = $($crate::net::device::feature::$f |)* 0;
        };
    }

    _netif_f!(NETIF_F_SG, NETIF_F_SG_BIT);
    _netif_f!(NETIF_F_FRAGLIST, NETIF_F_FRAGLIST_BIT);
    _netif_f!(NETIF_F_TSO, NETIF_F_TSO_BIT);
    _netif_f!(NETIF_F_TSO6, NETIF_F_TSO6_BIT);
    _netif_f!(NETIF_F_TSO_ECN, NETIF_F_TSO_ECN_BIT);
    _netif_f!(NETIF_F_TSO_MANGLEID, NETIF_F_TSO_MANGLEID_BIT);
    _netif_f!(NETIF_F_GSO_SCTP, NETIF_F_GSO_SCTP_BIT);
    _netif_f!(NETIF_F_GSO_UDP_L4, NETIF_F_GSO_UDP_L4_BIT);
    _netif_f!(NETIF_F_GSO_FRAGLIST, NETIF_F_GSO_FRAGLIST_BIT);
    _netif_f!(NETIF_F_HW_CSUM, NETIF_F_HW_CSUM_BIT);
    _netif_f!(NETIF_F_HIGHDMA, NETIF_F_HIGHDMA_BIT);
    _netif_f!(NETIF_F_LLTX, NETIF_F_LLTX_BIT);
    _netif_f!(NETIF_F_GSO_GRE, NETIF_F_GSO_GRE_BIT);
    _netif_f!(NETIF_F_GSO_GRE_CSUM, NETIF_F_GSO_GRE_CSUM_BIT);
    _netif_f!(NETIF_F_GSO_IPXIP4, NETIF_F_GSO_IPXIP4_BIT);
    _netif_f!(NETIF_F_GSO_IPXIP6, NETIF_F_GSO_IPXIP6_BIT);
    _netif_f!(NETIF_F_GSO_UDP_TUNNEL, NETIF_F_GSO_UDP_TUNNEL_BIT);
    _netif_f!(NETIF_F_GSO_UDP_TUNNEL_CSUM, NETIF_F_GSO_UDP_TUNNEL_CSUM_BIT);

    _netif_f_sum!(
        NETIF_F_ALL_TSO,
        NETIF_F_TSO,
        NETIF_F_TSO6,
        NETIF_F_TSO_ECN,
        NETIF_F_TSO_MANGLEID
    );
    _netif_f_sum!(
        NETIF_F_GSO_SOFTWARE,
        NETIF_F_ALL_TSO,
        NETIF_F_GSO_SCTP,
        NETIF_F_GSO_UDP_L4,
        NETIF_F_GSO_FRAGLIST
    );
    _netif_f_sum!(
        NETIF_F_GSO_ENCAP_ALL,
        NETIF_F_GSO_GRE,
        NETIF_F_GSO_GRE_CSUM,
        NETIF_F_GSO_IPXIP4,
        NETIF_F_GSO_IPXIP6,
        NETIF_F_GSO_UDP_TUNNEL,
        NETIF_F_GSO_UDP_TUNNEL_CSUM
    );
}

/// Helper functions for NetDevices.
pub mod helpers {
    use super::*;

    /// Validate the eth addres for the [`NetDevice`] `dev`.
    pub fn eth_validate_addr<T: NetDeviceAdapter>(dev: &NetDevice<T>) -> Result {
        // SAFETY: dev.ptr is valid if dev is valid
        let ret =
            unsafe { bindings::eth_validate_addr(dev.get_pointer() as *mut bindings::net_device) };
        if ret != 0 {
            Err(Error::from_kernel_errno(ret))
        } else {
            Ok(())
        }
    }

    /// Set new Ethernet hardware address.
    ///
    /// This doesn't change hardware matching, so needs to be overridden
    /// for most real devices.
    pub fn eth_mac_addr<T: NetDeviceAdapter>(
        dev: &mut NetDevice<T>,
        socket_addr: *mut c_types::c_void,
    ) -> Result {
        // SAFETY: dev.ptr is valid if dev is valid
        let ret = unsafe {
            bindings::eth_mac_addr(dev.get_pointer() as *mut bindings::net_device, socket_addr)
        };

        if ret != 0 {
            Err(Error::from_kernel_errno(ret))
        } else {
            Ok(())
        }
    }
}
