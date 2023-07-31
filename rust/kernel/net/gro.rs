//! Module for the Generic Receiver Offload
//!
//! C headers: [`include/net/gro.h`](../../../include/net/gro.h),

use crate::{
    bindings,
    net::{Device, SkBuff},
};
use core::marker::PhantomData;
use macros::vtable;

/// Abstraction around the kernel's `struct napi_struct`
///
/// For additional documentation about how New API (NAPI) works, consult the
/// [`Linux Foundation Wiki`](https://wiki.linuxfoundation.org/networking/napi).
#[repr(transparent)]
#[derive(Default, Clone, Copy)]
pub struct Napi(bindings::napi_struct);

/// A trait to implement NAPI Polling Functions
#[vtable]
pub trait NapiPoller {
    /// Polling function
    ///
    /// The NAPI structure is given mutably. The network driver should do its
    /// best to receive packets from the interface, and attempt to retrieve at
    /// most `budget` packets, returning the exact number it extracted.
    fn poll(_napi: &mut Napi, _budget: i32) -> i32 {
        // Budget is made into an `i32` because nothing in the kernel forbids
        // drivers from an explicit call to napi_poll(), and their polling
        // functions could return negative values for reasons only they know of.
        0
    }
}

/// Building structure for poller functions
pub struct PollerBuilder<T: NapiPoller> {
    _p: PhantomData<T>,
}

type PollerFunction = unsafe extern "C" fn(*mut bindings::napi_struct, i32) -> i32;

impl<T: NapiPoller> PollerBuilder<T> {
    const FUNC: Option<PollerFunction> = Some(Self::poller_callback);

    /// Build the poller function pointer associated with the generics' callback
    pub const fn build_function() -> Option<PollerFunction> {
        Self::FUNC
    }

    unsafe extern "C" fn poller_callback(napi: *mut bindings::napi_struct, budget: i32) -> i32 {
        // Try and build the napi from this pointer
        // SAFETY: The kernel will necessarily give us a non-null and valid
        // pointer, so we can dereference it, and use it while satisfying the
        // invariants of `Napi`. Furthermore, the cast is valid because `Napi`
        // is transparent.
        let napi: &mut Napi = unsafe { &mut *napi.cast() };

        // The rest is primitive, hence, trivial
        <T>::poll(napi, budget)
    }
}

impl Napi {
    /// Create a new, empty, NAPI
    pub fn new() -> Self {
        Self(bindings::napi_struct::default())
    }

    /// Obtain the inner pointer cast to the bindings type
    fn get_inner_cast(&mut self) -> *mut bindings::napi_struct {
        (self as *mut Self).cast()
    }

    /// Set a bit in the state bitmap of the [`Napi`] to 1
    pub fn set_state_bit(&mut self, bit: NapiState) {
        let bit_as = u64::from(bit as u32);

        self.0.state |= 1 << bit_as;
    }

    /// Enable the NAPI
    ///
    /// You must always set a state using [`Self::set_state_bit`] prior to
    /// calling this method.
    pub fn enable(&mut self) {
        let napi_ptr: *mut Napi = self;

        // SAFETY: The cast is valid because `Napi` is transparent to that type,
        // and the call is sound because the pointer is guaranteed to be
        // non-null and valid all throughout the lifetime of the call.
        unsafe { bindings::napi_enable(napi_ptr.cast()) };
    }

    /// Disable the NAPI
    pub fn disable(&mut self) {
        let napi_ptr: *mut Napi = self;

        // SAFETY: The cast is valid because `Napi` is transparent to that type,
        // and the call is sound because the pointer is guaranteed to be
        // non-null and valid all throughout the lifetime of the call.
        unsafe { bindings::napi_disable(napi_ptr.cast()) };
    }

    /// Schedule the NAPI to run on this CPU
    ///
    /// This is equivalent to calling [`Self::prepare_scheduling`] followed by
    /// [`Self::actually_schedule`] one after the other.
    pub fn schedule(&mut self) {
        // SAFETY: The call is safe because the pointer is guaranteed to be
        // non-null and valid all throughout the call.
        unsafe { bindings::napi_schedule(self.get_inner_cast()) };
    }

    /// Prepare the scheduling of the NAPI
    ///
    /// If the NAPI is already due to be scheduled on this CPU, do nothing
    /// and return `false`.
    ///
    /// Call [`Self::actually_schedule`] if this method returns `true`.
    pub fn prepare_scheduling(&mut self) -> bool {
        // SAFETY: The call is safe because the pointer is guaranteed to be
        // non-null and valid all throughout the call.
        unsafe { bindings::napi_schedule_prep(self.get_inner_cast()) }
    }

    /// Actually schedule the NAPI after preparation
    ///
    /// Call [`Self::prepare_scheduling`] prior to calling this method.
    pub fn actually_schedule(&mut self) {
        // SAFETY: The call is safe because the pointer is guaranteed to be
        // non-null and valid all throughout the call.
        unsafe { bindings::__napi_schedule(self.get_inner_cast()) };
    }

    /// Complete after no packets received by the NAPI
    ///
    /// This is equivalent to calling [`Self::complete_done`] with a work of 0.
    pub fn complete(&mut self) -> bool {
        // SAFETY: The call is safe because the pointer is guaranteed to be
        // non-null and valid all throughout the call
        unsafe { bindings::napi_complete_done(self.get_inner_cast(), 0) }
    }

    /// Complete with a given number of packets received by the NAPI
    pub fn complete_done(&mut self, work: i32) -> bool {
        // SAFETY: The call is safe because `work` is primitive, and the pointer
        // is guaranteed to be non-null and valid throughout the call's
        // lifetime.
        unsafe { bindings::napi_complete_done(self.get_inner_cast(), work) }
    }

    /// Return a reference to the device that the NAPI is currently on, if any
    pub fn get_device(&self) -> Option<&Device> {
        let dev_ptr = self.0.dev;
        if dev_ptr.is_null() {
            None
        } else {
            // SAFETY: We've guaranteed that `dev_ptr` is non-null. The kernel
            // guarantees that it's a pointer to a net_device, and it will stay
            // valid for the duration of the instance given here.
            Some(unsafe { Device::from_ptr(dev_ptr) })
        }
    }

    /// Transmit to the GRO
    pub fn gro_receive(&mut self, sk_buff: &mut SkBuff) -> GroResult {
        let self_ptr = self.get_inner_cast();
        let skb_ptr: *mut bindings::sk_buff = (sk_buff as *mut SkBuff).cast();

        // SAFETY: The invariants of SkBuff and ourself guarantees that we can
        // use these pointers.
        let res = unsafe { bindings::napi_gro_receive(self_ptr, skb_ptr) };
        res.try_into()
            .expect("Unable to convert return of napi_gro_receive to gro_result\n")
    }
}

/// Enumerator for the return type of [`SkBuff::gro_receive`]
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum GroResult {
    /// Merged but not freed
    Merged = bindings::gro_result_GRO_MERGED,

    /// Merged and freed
    MergedFree = bindings::gro_result_GRO_MERGED_FREE,

    /// Held
    Held = bindings::gro_result_GRO_HELD,

    /// Normal
    Normal = bindings::gro_result_GRO_NORMAL,

    /// Consumed
    Consumed = bindings::gro_result_GRO_CONSUMED,
}

impl TryFrom<u32> for GroResult {
    type Error = ();
    fn try_from(u: u32) -> core::result::Result<Self, Self::Error> {
        match u {
            bindings::gro_result_GRO_MERGED => Ok(Self::Merged),
            bindings::gro_result_GRO_MERGED_FREE => Ok(Self::MergedFree),
            bindings::gro_result_GRO_HELD => Ok(Self::Held),
            bindings::gro_result_GRO_NORMAL => Ok(Self::Normal),
            bindings::gro_result_GRO_CONSUMED => Ok(Self::Consumed),
            _ => Err(()),
        }
    }
}

/// Enumerator for the state of a [`Napi`]
///
/// The state of a [`Napi`] must always be set prior to enabling it.
#[repr(u32)]
pub enum NapiState {
    /// Poll is scheduled
    Sched = bindings::NAPI_STATE_SCHED,

    /// Rescheduling
    Missed = bindings::NAPI_STATE_MISSED,

    /// Disable is pending
    Disable = bindings::NAPI_STATE_DISABLE,

    /// Netpoll - don't dequeue from poll_list
    Npsvc = bindings::NAPI_STATE_NPSVC,

    /// NAPI added to system list
    Listed = bindings::NAPI_STATE_LISTED,

    /// Do not add in napi_hash, no busy polling
    NoBusyPoll = bindings::NAPI_STATE_NO_BUSY_POLL,

    /// `sk_busy_loop()` owns this NAPI
    InBusyPoll = bindings::NAPI_STATE_IN_BUSY_POLL,

    /// Prefer busy-polling over softirqd processing
    PreferBusyPoll = bindings::NAPI_STATE_PREFER_BUSY_POLL,

    /// The poll is performed inside its own thread
    Threaded = bindings::NAPI_STATE_THREADED,

    /// NAPI is currently scheduled in threaded mode
    SchedThreaded = bindings::NAPI_STATE_SCHED_THREADED,
}

impl From<NapiState> for u32 {
    fn from(n: NapiState) -> u32 {
        n as u32
    }
}
