// SPDX-License-Identifier: GPL-2.0-only

//! Congestion control algorithms (CCA).
//!
//! Abstractions for implementing pluggable CCAs in Rust.

use crate::bindings;
use crate::error::{self, Error, VTABLE_DEFAULT_ERROR};
use crate::init::PinInit;
use crate::net::sock;
use crate::prelude::{pr_err, vtable};
use crate::str::CStr;
use crate::time;
use crate::types::Opaque;
use crate::ThisModule;
use crate::{build_assert, build_error, field_size, try_pin_init};
use core::convert::TryFrom;
use core::marker::PhantomData;
use core::pin::Pin;
use macros::{pin_data, pinned_drop};

use super::{InetConnectionSock, TcpSock};

pub mod hystart;

/// Congestion control algorithm (CCA).
///
/// A CCA is implemented as a set of callbacks that are invoked whenever
/// specific events occur in a connection. Each socket has its own instance of
/// some CCA. Every instance of a CCA has its own private data that is stored in
/// the socket and is mutated by the callbacks.
///
/// Callbacks that operate on the same instance are guaranteed to run
/// sequentially, and each callback has exclusive mutable access to the private
/// data of the instance it operates on.
#[vtable]
pub trait Algorithm {
    /// Private data. Each socket has its own instance.
    type Data: Default + Send + Sync;

    /// Name of the algorithm.
    const NAME: &'static CStr;

    /// Called when entering CWR, Recovery, or Loss states from Open or Disorder
    /// states. Returns the new slow start threshold.
    fn ssthresh(sk: &mut Sock<'_, Self>) -> u32;

    /// Called when one of the events in [`Event`] occurs.
    fn cwnd_event(_sk: &mut Sock<'_, Self>, _ev: Event) {
        build_error!(VTABLE_DEFAULT_ERROR);
    }

    /// Called towards the end of processing an ACK if a cwnd increase is
    /// possible. Performs a new cwnd calculation and sets it on the socket.
    // Note: In fact, one of `cong_avoid` and `cond_control` is required.
    // (see `tcp_validate_congestion_control`)
    fn cong_avoid(sk: &mut Sock<'_, Self>, ack: u32, acked: u32);

    /// Called before the sender's congestion state is changed.
    fn set_state(_sk: &mut Sock<'_, Self>, _new_state: State) {
        build_error!(VTABLE_DEFAULT_ERROR);
    }

    /// Called when removing ACKed packets from the retransmission queue. Can be
    /// used for packet ACK accounting.
    fn pkts_acked(_sk: &mut Sock<'_, Self>, _sample: &AckSample) {
        build_error!(VTABLE_DEFAULT_ERROR);
    }

    /// Called to undo a recent cwnd reduction that was found to has been
    /// unnecessary. Returns the new value of cwnd.
    fn undo_cwnd(sk: &mut Sock<'_, Self>) -> u32;

    /// Initializes the private data.
    ///
    /// When this function is called, [`sk.inet_csk_ca()`] will contain a value
    /// returned by `Self::Data::default()`.
    ///
    /// Only implement this function when you need to perform additional setup
    /// tasks.
    ///
    /// [`sk.inet_csk_ca()`]: Sock::inet_csk_ca
    fn init(_sk: &mut Sock<'_, Self>) {
        build_error!(VTABLE_DEFAULT_ERROR);
    }

    /// Cleans up the private data.
    ///
    /// After this function returns, [`sk.inet_csk_ca()`] will be dropped.
    ///
    /// Only implement this function when you need to perform additional cleanup
    /// tasks.
    ///
    /// [`sk.inet_csk_ca()`]: Sock::inet_csk_ca
    fn release(_sk: &mut Sock<'_, Self>) {
        build_error!(VTABLE_DEFAULT_ERROR);
    }
}

pub mod reno {
    //! TCP Reno congestion control.
    //!
    //! Algorithms may choose to invoke these callbacks instead of providing
    //! their own implementation. This is convenient as a new CCA might have
    //! the same logic as an existing one in some of its callbacks.
    use super::{Algorithm, Sock};
    use crate::bindings;

    /// Implementation of [`undo_cwnd`] that returns `max(snd_cwnd, prior_cwnd)`,
    /// where `prior_cwnd` is the value of cwnd before the last reduction.
    ///
    /// [`undo_cwnd`]: super::Algorithm::undo_cwnd
    #[inline]
    pub fn undo_cwnd<T: Algorithm + ?Sized>(sk: &mut Sock<'_, T>) -> u32 {
        // SAFETY:
        // - `sk` has been passed to the callback that invoked us,
        // - it is OK to pass it to the callback of the Reno algorithm as it
        //   will never touch the private data.
        unsafe { bindings::tcp_reno_undo_cwnd(sk.sk.raw_sk_mut()) }
    }
}

/// Representation of the `struct sock *` that is passed to the callbacks of the
/// CCA.
///
/// Every callback receives a pointer to the socket that it is operating on.
/// There are certain operations that callbacks are allowed to perform on the
/// socket, and this type just exposes methods for performing those. This
/// prevents callbacks from performing arbitrary manipulations on the socket.
//  TODO: Currently all callbacks can perform all operations. However, this
//  might be too permissive, e.g., the `pkts_acked` callback should probably not
//  be changing cwnd...
///
/// # Invariants
///
/// The wrapped `sk` must have been obtained as the argument to a callback of
/// the congestion algorithm `T` (other than the `init` cb) and may only be used
/// for the duration of that callback. In particular:
///
/// - `sk` points to a valid `struct sock`.
/// - `tcp_sk(sk)` points to a valid `struct tcp_sock`.
/// - The socket uses the CCA `T`.
/// - `inet_csk_ca(sk)` points to a valid instance of `T::Data`, which belongs
///   to the instance of the algorithm used by this socket. A callback has
///   exclusive, mutable access to this data.
pub struct Sock<'a, T: Algorithm + ?Sized> {
    sk: &'a mut sock::Sock,
    _pd: PhantomData<T::Data>,
}

impl<'a, T: Algorithm + ?Sized> Sock<'a, T> {
    /// Creates a new `Sock`.
    ///
    /// # Safety
    ///
    /// - `sk` must have been obtained as the argument to a callback of the
    ///   congestion algorithm `T`.
    /// - The CCAs private data must have been initialised.
    /// - The returned value must not live longer than the duration of the
    ///   callback.
    unsafe fn new(sk: *mut bindings::sock) -> Self {
        // INVARIANTS: Satisfied by the functions precondition.
        Self {
            // SAFETY:
            // - The cast is OK since `sock::Sock` is transparent to
            //   `struct sock`.
            // - Dereferencing `sk` is OK since the pointers passed to CCA CBs
            //   are valid.
            // - By the function's preconditions, the produced `Self` value will
            //   only live for the duration of the callback; thus, the wrapped
            //   reference will always be valid.
            sk: unsafe { &mut *(sk as *mut sock::Sock) },
            _pd: PhantomData,
        }
    }

    /// Returns the [`TcpSock`] that is containing the `Sock`.
    #[inline]
    pub fn tcp_sk<'b>(&'b self) -> &'b TcpSock {
        // SAFETY: By the type invariants, `sk` is valid for `tcp_sk`.
        unsafe { self.sk.tcp_sk() }
    }

    /// Returns the [`TcpSock`] that is containing the `Sock`.
    #[inline]
    pub fn tcp_sk_mut<'b>(&'b mut self) -> &'b mut TcpSock {
        // SAFETY: By the type invariants, `sk` is valid for `tcp_sk`.
        unsafe { self.sk.tcp_sk_mut() }
    }

    /// Returns the [private data] of the instance of the CCA used by this
    /// socket.
    ///
    /// [private data]: Algorithm::Data
    #[inline]
    pub fn inet_csk_ca<'b>(&'b self) -> &'b T::Data {
        // SAFETY: By the type invariants, `sk` is valid for `inet_csk_ca`, it
        // it uses the algorithm `T`, and the private data is valid.
        unsafe { self.sk.inet_csk_ca::<T>() }
    }

    /// Returns the [private data] of the instance of the CCA used by this
    /// socket.
    ///
    /// [private data]: Algorithm::Data
    #[inline]
    pub fn inet_csk_ca_mut<'b>(&'b mut self) -> &'b mut T::Data {
        // SAFETY: By the type invariants, `sk` is valid for `inet_csk_ca`, it
        // it uses the algorithm `T`, and the private data is valid.
        unsafe { self.sk.inet_csk_ca_mut::<T>() }
    }

    /// Returns the [`InetConnectionSock`] of this socket.
    #[inline]
    pub fn inet_csk<'b>(&'b self) -> &'b InetConnectionSock {
        // SAFETY: By the type invariants, `sk` is valid for `inet_csk`.
        unsafe { self.sk.inet_csk() }
    }

    /// Tests if the connection's sending rate is limited by the cwnd.
    // NOTE: This feels like it should be a method on `TcpSock`, but C defines
    // it on `struct sock` so there is not much we can do about it. At least, if
    // we don't want to reimplement the function (or perform the conversion from
    // `struct tcp_sock` to `struct sock` just to have C reverse it right away.
    #[inline]
    pub fn tcp_is_cwnd_limited(&self) -> bool {
        // SAFETY: By the type invariants, `sk` is valid for
        // `tcp_is_cwnd_limited`.
        unsafe { self.sk.tcp_is_cwnd_limited() }
    }

    /// Returns the sockets pacing rate in bytes per second.
    #[inline]
    pub fn sk_pacing_rate(&self) -> u64 {
        self.sk.sk_pacing_rate()
    }

    /// Returns the sockets pacing status.
    #[inline]
    pub fn sk_pacing_status(&self) -> Result<sock::Pacing, ()> {
        self.sk.sk_pacing_status()
    }

    /// Returns the sockets maximum GSO segment size to build.
    #[inline]
    pub fn sk_gso_max_size(&self) -> u32 {
        self.sk.sk_gso_max_size()
    }
}

/// Representation of the `struct ack_sample *` that is passed to the
/// `pkts_acked` callback of the CCA.
///
/// # Invariants
///
/// - `sample` points to a valid `struct ack_sample`,
/// - all fields of `sample` can be read without additional synchronization.
pub struct AckSample {
    sample: *const bindings::ack_sample,
}

impl AckSample {
    /// Creates a new `AckSample`.
    ///
    /// # Safety
    ///
    /// `sample` must have been obtained as the argument to the `pkts_acked`
    /// callback.
    unsafe fn new(sample: *const bindings::ack_sample) -> Self {
        // INVARIANTS: Satisfied by the function's precondition.
        Self { sample }
    }

    /// Returns the number of packets that were ACKed.
    #[inline]
    pub fn pkts_acked(&self) -> u32 {
        // SAFETY: By the type invariants it is OK to read any field.
        unsafe { (*self.sample).pkts_acked }
    }

    /// Returns the RTT measurement of this ACK sample.
    // Note: Some samples might not include a RTT measurement. This is indicated
    // by a negative value for `rtt_us`, we return `None` in that case.
    #[inline]
    pub fn rtt_us(&self) -> Option<time::Usecs32> {
        // SAFETY: By the type invariants it is OK to read any field.
        match unsafe { (*self.sample).rtt_us } {
            t if t < 0 => None,
            t => Some(t as time::Usecs32),
        }
    }
}

/// States of the TCP sender state machine.
///
/// The TCP sender's congestion state indicating normal or abnormal situations
/// in the last round of packets sent. The state is driven by the ACK
/// information and timer events.
#[repr(u8)]
pub enum State {
    /// Nothing bad has been observed recently. No apparent reordering, packet
    /// loss, or ECN marks.
    Open = bindings::tcp_ca_state_TCP_CA_Open as u8,
    /// The sender enters disordered state when it has received DUPACKs or
    /// SACKs in the last round of packets sent. This could be due to packet
    /// loss or reordering but needs further information to confirm packets
    /// have been lost.
    Disorder = bindings::tcp_ca_state_TCP_CA_Disorder as u8,
    /// The sender enters Congestion Window Reduction (CWR) state when it
    /// has received ACKs with ECN-ECE marks, or has experienced congestion
    /// or packet discard on the sender host (e.g. qdisc).
    Cwr = bindings::tcp_ca_state_TCP_CA_CWR as u8,
    /// The sender is in fast recovery and retransmitting lost packets,
    /// typically triggered by ACK events.
    Recovery = bindings::tcp_ca_state_TCP_CA_Recovery as u8,
    /// The sender is in loss recovery triggered by retransmission timeout.
    Loss = bindings::tcp_ca_state_TCP_CA_Loss as u8,
}

// TODO: Replace with automatically generated code by bindgen when it becomes
// possible.
impl TryFrom<u8> for State {
    type Error = ();

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            x if x == State::Open as u8 => Ok(State::Open),
            x if x == State::Disorder as u8 => Ok(State::Disorder),
            x if x == State::Cwr as u8 => Ok(State::Cwr),
            x if x == State::Recovery as u8 => Ok(State::Recovery),
            x if x == State::Loss as u8 => Ok(State::Loss),
            _ => Err(()),
        }
    }
}

/// Events passed to congestion control interface.
#[repr(u32)]
pub enum Event {
    /// First transmit when no packets in flight.
    TxStart = bindings::tcp_ca_event_CA_EVENT_TX_START,
    /// Congestion window restart.
    CwndRestart = bindings::tcp_ca_event_CA_EVENT_CWND_RESTART,
    /// End of congestion recovery.
    CompleteCwr = bindings::tcp_ca_event_CA_EVENT_COMPLETE_CWR,
    /// Loss timeout.
    Loss = bindings::tcp_ca_event_CA_EVENT_LOSS,
    /// ECT set, but not CE marked.
    EcnNoCe = bindings::tcp_ca_event_CA_EVENT_ECN_NO_CE,
    /// Received CE marked IP packet.
    EcnIsCe = bindings::tcp_ca_event_CA_EVENT_ECN_IS_CE,
}

// TODO: Replace with automatically generated code by bindgen when it becomes
// possible.
impl TryFrom<bindings::tcp_ca_event> for Event {
    type Error = ();

    fn try_from(ev: bindings::tcp_ca_event) -> Result<Self, Self::Error> {
        match ev {
            x if x == Event::TxStart as u32 => Ok(Event::TxStart),
            x if x == Event::CwndRestart as u32 => Ok(Event::CwndRestart),
            x if x == Event::CompleteCwr as u32 => Ok(Event::CompleteCwr),
            x if x == Event::Loss as u32 => Ok(Event::Loss),
            x if x == Event::EcnNoCe as u32 => Ok(Event::EcnNoCe),
            x if x == Event::EcnIsCe as u32 => Ok(Event::EcnIsCe),
            _ => Err(()),
        }
    }
}

#[pin_data(PinnedDrop)]
struct Registration<T: Algorithm + ?Sized> {
    #[pin]
    ops: Opaque<bindings::tcp_congestion_ops>,
    _pd: PhantomData<T>,
}

// SAFETY: `Registration` doesn't provide any `&self` methods, so it is safe to
// pass references to it around.
unsafe impl<T: Algorithm + ?Sized> Sync for Registration<T> {}

// SAFETY: Both registration and unregistration are implemented in C and safe to
// be performed from any thread, so `Registration` is `Send`.
unsafe impl<T: Algorithm + ?Sized> Send for Registration<T> {}

impl<T: Algorithm + ?Sized> Registration<T> {
    const NAME_FIELD: [i8; 16] = Self::gen_name_field::<16>();
    // Maximal size of the private data.
    const ICSK_CA_PRIV_SIZE: usize = field_size!(bindings::inet_connection_sock, icsk_ca_priv);
    const DATA_SIZE: usize = core::mem::size_of::<T::Data>();

    fn new(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            _pd: PhantomData,
            ops <- Opaque::try_ffi_init(|ops_ptr: *mut bindings::tcp_congestion_ops| {
                // SAFETY: `try_ffi_init` guarantees that `ops_ptr` is valid for
                // write.
                unsafe { ops_ptr.write(bindings::tcp_congestion_ops::default()) };

                // SAFETY: `try_ffi_init` guarantees that `ops_ptr` is valid for
                // write, and it has just been initialised above, so it's also
                // valid for read.
                let ops = unsafe { &mut *ops_ptr };

                ops.ssthresh = Some(Self::ssthresh_cb);
                ops.cong_avoid = Some(Self::cong_avoid_cb);
                ops.undo_cwnd = Some(Self::undo_cwnd_cb);
                if T::HAS_SET_STATE {
                    ops.set_state = Some(Self::set_state_cb);
                }
                if T::HAS_PKTS_ACKED {
                    ops.pkts_acked = Some(Self::pkts_acked_cb);
                }
                if T::HAS_CWND_EVENT {
                    ops.cwnd_event = Some(Self::cwnd_event_cb);
                }

                // Even though it is not mandated by the C side, we
                // unconditionally set these CBs to ensure that it is always
                // safe to access the CCA's private data.
                // Future work could allow the CCA to declare whether it wants
                // to be able to use the private data.
                ops.init = Some(Self::init_cb);
                ops.release = Some(Self::release_cb);

                ops.owner = module.0;
                ops.name = Self::NAME_FIELD;

                // SAFETY: Pointers stored in `ops` are static so they will live
                // for as long as the registration is active (it is undone in
                // `drop`).
                error::to_result( unsafe { bindings::tcp_register_congestion_control(ops_ptr) })
            }),
        })
    }

    const fn gen_name_field<const N: usize>() -> [i8; N] {
        let mut name_field: [i8; N] = [0; N];
        let mut i = 0;

        while i < T::NAME.len_with_nul() {
            name_field[i] = T::NAME.as_bytes_with_nul()[i] as i8;
            i += 1;
        }

        name_field
    }

    unsafe extern "C" fn cwnd_event_cb(sk: *mut bindings::sock, ev: bindings::tcp_ca_event) {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        match Event::try_from(ev) {
            Ok(ev) => T::cwnd_event(&mut sk, ev),
            Err(_) => pr_err!("cwnd_event: event was {}", ev),
        }
    }

    unsafe extern "C" fn init_cb(sk: *mut bindings::sock) {
        // Fail the build if the module-defined private data is larger than the
        // storage that the kernel provides.
        build_assert!(Self::DATA_SIZE <= Self::ICSK_CA_PRIV_SIZE);

        // SAFETY:
        // - The `sk` that is passed to this callback is valid for
        //   `inet_csk_ca`.
        // - We just checked that there is enough space for the cast to be okay.
        let ca = unsafe { bindings::inet_csk_ca(sk) as *mut T::Data };

        unsafe { ca.write(T::Data::default()) };

        if T::HAS_INIT {
            // SAFETY:
            // - `sk` was passed to a callback of the CCA `T`.
            // - We just initialized the `Data`.
            // - This value will be dropped at the end of the callback.
            let mut sk = unsafe { Sock::new(sk) };
            T::init(&mut sk)
        }
    }

    unsafe extern "C" fn release_cb(sk: *mut bindings::sock) {
        if T::HAS_RELEASE {
            // SAFETY:
            // - `sk` was passed to a callback of the CCA `T`.
            // - `Data` is guaranteed to be initialized since the `init_cb` took
            //   care of it.
            // - This value will be dropped at the end of the callback.
            let mut sk = unsafe { Sock::new(sk) };
            T::release(&mut sk)
        }

        // We have to manually dispose the private data that we stored with the
        // kernel.
        // SAFETY:
        // - The `sk` passed to callbacks is valid for `inet_csk_ca`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - After we return no other callback will be invoked with this socket.
        unsafe { core::ptr::drop_in_place(bindings::inet_csk_ca(sk) as *mut T::Data) };
    }

    unsafe extern "C" fn ssthresh_cb(sk: *mut bindings::sock) -> u32 {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        T::ssthresh(&mut sk)
    }

    unsafe extern "C" fn cong_avoid_cb(sk: *mut bindings::sock, ack: u32, acked: u32) {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        T::cong_avoid(&mut sk, ack, acked)
    }

    unsafe extern "C" fn set_state_cb(sk: *mut bindings::sock, new_state: u8) {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        match State::try_from(new_state) {
            Ok(new_state) => T::set_state(&mut sk, new_state),
            Err(_) => pr_err!("set_state: new_state was {}", new_state),
        }
    }

    unsafe extern "C" fn pkts_acked_cb(
        sk: *mut bindings::sock,
        sample: *const bindings::ack_sample,
    ) {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        // SAFETY:
        // - `sample` points to a valid `struct ack_sample`.
        let sample = unsafe { AckSample::new(sample) };
        T::pkts_acked(&mut sk, &sample)
    }

    unsafe extern "C" fn undo_cwnd_cb(sk: *mut bindings::sock) -> u32 {
        // SAFETY:
        // - `sk` was passed to a callback of the CCA `T`.
        // - `Data` is guaranteed to be initialized since the `init_cb` took
        //   care of it.
        // - This value will be dropped at the end of the callback.
        let mut sk = unsafe { Sock::new(sk) };
        T::undo_cwnd(&mut sk)
    }
}

#[pinned_drop]
impl<T: Algorithm + ?Sized> PinnedDrop for Registration<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY:
        // - The fact that `Self` exists implies that a previous call to
        //   `tcp_register_congestion_control` with `self.ops.get()` was
        //   successful.
        unsafe { bindings::tcp_unregister_congestion_control(self.ops.get()) };
    }
}

/// Kernel module that implements a single CCA `T`.
#[pin_data]
pub struct Module<T: Algorithm + ?Sized> {
    #[pin]
    reg: Registration<T>,
}

impl<T: Algorithm + ?Sized + Sync + Send> crate::InPlaceModule for Module<T> {
    fn init(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            reg <- Registration::<T>::new(module),
        })
    }
}

/// Defines a kernel module that implements a single congestion control
/// algorithm.
///
/// # Examples
///
/// To start experimenting with your own congestion control algorithm, implement
/// the [`Algorithm`] trait and use this macro to declare the module to the
/// rest of the kerne. That's it!
///
/// ```ignore
/// use kernel::{c_str, module_cca};
/// use kernel::prelude::*;
/// use kernel::net::tcp::cong::*;
/// use core::num::NonZeroU32;
///
/// struct MyCca {}
///
/// #[vtable]
/// impl Algorithm for MyCca {
///     type Data = ();
///
///     const NAME: &'static CStr = c_str!("my_cca");
///
///     fn undo_cwnd(sk: &mut Sock<'_, Self>) -> u32 {
///         reno::undo_cwnd(sk)
///     }
///
///     fn ssthresh(_sk: &mut Sock<'_, Self>) -> u32 {
///         2
///     }
///
///     fn cong_avoid(sk: &mut Sock<'_, Self>, _ack: u32, acked: u32) {
///         sk.tcp_sk_mut().cong_avoid_ai(NonZeroU32::new(1).unwrap(), acked)
///     }
/// }
///
/// module_cca! {
///     type: MyCca,
///     name: "my_cca",
///     author: "Rust for Linux Contributors",
///     description: "Sample congestion control algorithm implemented in Rust.",
///     license: "GPL v2",
/// }
/// ```
#[macro_export]
macro_rules! module_cca {
    (type: $type:ty, $($f:tt)*) => {
        type ModuleType = $crate::net::tcp::cong::Module<$type>;
        $crate::macros::module! {
            type: ModuleType,
            $($f)*
        }
    }
}
pub use module_cca;
