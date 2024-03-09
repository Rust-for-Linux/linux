// SPDX-License-Identifier: GPL-2.0-only

//! TCP CUBIC congestion control algorithm.
//!
//! Based on:
//!     Sangtae Ha, Injong Rhee, and Lisong Xu. 2008.
//!     CUBIC: A New TCP-Friendly High-Speed TCP Variant.
//!     SIGOPS Oper. Syst. Rev. 42, 5 (July 2008), 64–74.
//!     <https://doi.org/10.1145/1400097.1400105>
//!
//! CUBIC is also described in [RFC9438](https://www.rfc-editor.org/rfc/rfc9438).

use core::cmp::{max, min};
use core::num::NonZeroU32;
use kernel::c_str;
use kernel::net::tcp;
use kernel::net::tcp::cong::{self, hystart, hystart::HystartDetect, module_cca};
use kernel::prelude::*;
use kernel::time;

const BICTCP_BETA_SCALE: u32 = 1024;

// TODO: Convert to module parameters once they are available. Currently these
// are the defaults from the C implementation.
// TODO: Use `NonZeroU32` where appropriate.
/// Whether to use fast convergence. This is a heuristic to increase the
/// release of bandwidth by existing flows to speed up the convergence to a
/// steady state when a new flow joins the link.
const FAST_CONVERGENCE: bool = true;
/// The factor for multiplicative decrease of cwnd upon a loss event. Will be
/// divided by `BICTCP_BETA_SCALE`, approximately 0.7.
const BETA: u32 = 717;
/// The initial value of ssthresh for new connections. Setting this to `None`
/// implies `i32::MAX`.
const INITIAL_SSTHRESH: Option<u32> = None;
/// The parameter `C` that scales the cubic term is defined as `BIC_SCALE/2^10`.
/// (For C: Dimension: Time^-2, Unit: s^-2).
const BIC_SCALE: u32 = 41;
/// In environments where CUBIC grows cwnd less aggressively than normal TCP,
/// enabling this option causes it to behave like normal TCP instead. This is
/// the case in short RTT and/or low bandwidth delay product networks.
const TCP_FRIENDLINESS: bool = true;
/// Whether to use the [HyStart] slow start algorithm.
///
/// [HyStart]: hystart::HyStart
const HYSTART: bool = true;

impl hystart::HyStart for Cubic {
    /// Which mechanism to use for deciding when it is time to exit slow start.
    const DETECT: HystartDetect = HystartDetect::Both;
    /// Lower bound for cwnd during hybrid slow start.
    const LOW_WINDOW: u32 = 16;
    /// Spacing between ACKs indicating an ACK-train.
    /// (Dimension: Time. Unit: us).
    const ACK_DELTA: time::Usecs32 = 2000;
}

// TODO: Those are computed based on the module parameters in the init. Even
// with module parameters available this will be a bit tricky to do in Rust.
/// Factor of `8/3 * (1 + beta) / (1 - beta)` that is used in various
/// calculations. (Dimension: none)
const BETA_SCALE: u32 = ((8 * (BICTCP_BETA_SCALE + BETA)) / 3) / (BICTCP_BETA_SCALE - BETA);
/// Factor of `2^10*C/SRTT` where `SRTT = 100ms` that is used in various
/// calculations. (Dimension: Time^-3, Unit: s^-3).
const CUBE_RTT_SCALE: u32 = BIC_SCALE * 10;
/// Factor of `SRTT/C` where `SRTT = 100ms` and `C` from above.
/// (Dimension: Time^3. Unit: (ms)^3)
// Note: C uses a custom time unit of 2^-10 s called `BICTCP_HZ`. This
// implementation consistently uses milliseconds instead.
const CUBE_FACTOR: u64 = 1_000_000_000 * (1u64 << 10) / (CUBE_RTT_SCALE as u64);

module_cca! {
    type: Cubic,
    name: "tcp_cubic_rust",
    author: "Rust for Linux Contributors",
    description: "TCP CUBIC congestion control algorithm, Rust implementation",
    license: "GPL v2",
}

struct Cubic {}

#[vtable]
impl cong::Algorithm for Cubic {
    type Data = CubicState;

    const NAME: &'static CStr = c_str!("cubic_rust");

    fn init(sk: &mut cong::Sock<'_, Self>) {
        if HYSTART {
            <Self as hystart::HyStart>::reset(sk)
        } else if let Some(ssthresh) = INITIAL_SSTHRESH {
            sk.tcp_sk_mut().set_snd_ssthresh(ssthresh);
        }

        // TODO: remove
        pr_info!(
            "init: socket created: start {}us",
            sk.inet_csk_ca().hystart_state.start_time
        );
    }

    // TODO: remove
    fn release(sk: &mut cong::Sock<'_, Self>) {
        pr_info!(
            "release: socket destroyed: start {}us, end {}us",
            sk.inet_csk_ca().hystart_state.start_time,
            time::ktime_get_boot_fast_us32(),
        );
    }

    fn cwnd_event(sk: &mut cong::Sock<'_, Self>, ev: cong::Event) {
        if matches!(ev, cong::Event::TxStart) {
            // Here we cannot avoid jiffies as the `lsndtime` field is measured
            // in jiffies.
            let now = time::jiffies32();
            let delta: time::Jiffies32 = now.wrapping_sub(sk.tcp_sk().lsndtime());

            if (delta as i32) <= 0 {
                return;
            }

            let ca = sk.inet_csk_ca_mut();
            // Ok, lets switch to SI units.
            let now = time::ktime_get_boot_fast_ms32();
            let delta = time::jiffies_to_msecs(delta as time::Jiffies);
            // TODO: remove
            pr_debug!("cwnd_event: TxStart, now {}ms, delta {}ms", now, delta);
            // We were application limited, i.e., idle, for a while. If we are
            // in congestion avoidance, shift `epoch_start` by the time we were
            // idle to keep cwnd growth to cubic curve.
            ca.epoch_start = ca.epoch_start.map(|mut epoch_start| {
                epoch_start = epoch_start.wrapping_add(delta);
                if tcp::after(epoch_start, now) {
                    epoch_start = now;
                }
                epoch_start
            });
        }
    }

    fn set_state(sk: &mut cong::Sock<'_, Self>, new_state: cong::State) {
        if matches!(new_state, cong::State::Loss) {
            pr_info!(
                // TODO: remove
                "set_state: Loss, time {}us, start {}us",
                time::ktime_get_boot_fast_us32(),
                sk.inet_csk_ca().hystart_state.start_time
            );
            sk.inet_csk_ca_mut().reset();
            <Self as hystart::HyStart>::reset(sk);
        }
    }

    fn pkts_acked(sk: &mut cong::Sock<'_, Self>, sample: &cong::AckSample) {
        // Some samples do not include RTTs.
        let Some(rtt_us) = sample.rtt_us() else {
            // TODO: remove
            pr_debug!(
                "pkts_acked: no RTT sample, start {}us",
                sk.inet_csk_ca().hystart_state.start_time,
            );
            return;
        };

        let epoch_start = sk.inet_csk_ca().epoch_start;
        // For some time after existing fast recovery the samples might still be
        // inaccurate.
        if epoch_start.is_some_and(|epoch_start| {
            time::ktime_get_boot_fast_ms32().wrapping_sub(epoch_start) < time::MSEC_PER_SEC
        }) {
            // TODO: remove
            pr_debug!(
                "pkts_acked: {}ms - {}ms < 1s, too close to epoch_start",
                time::ktime_get_boot_fast_ms32(),
                epoch_start.unwrap()
            );
            return;
        }

        let delay = max(1, rtt_us);
        let cwnd = sk.tcp_sk().snd_cwnd();
        let in_slow_start = sk.tcp_sk().in_slow_start();
        let ca = sk.inet_csk_ca_mut();

        // TODO: remove
        pr_debug!(
            "pkts_acked: delay {}us, cwnd {}, ss {}",
            delay,
            cwnd,
            in_slow_start
        );

        // First call after reset or the delay decreased.
        if ca.hystart_state.delay_min.is_none()
            || ca
                .hystart_state
                .delay_min
                .is_some_and(|delay_min| delay_min > delay)
        {
            ca.hystart_state.delay_min = Some(delay);
        }

        if in_slow_start && HYSTART && ca.hystart_state.in_hystart::<Self>(cwnd) {
            hystart::HyStart::update(sk, delay);
        }
    }

    fn ssthresh(sk: &mut cong::Sock<'_, Self>) -> u32 {
        let cwnd = sk.tcp_sk().snd_cwnd();
        let ca = sk.inet_csk_ca_mut();

        pr_info!(
            // TODO: remove
            "ssthresh: time {}us, start {}us",
            time::ktime_get_boot_fast_us32(),
            ca.hystart_state.start_time
        );

        // Epoch has ended.
        ca.epoch_start = None;
        ca.last_max_cwnd = if cwnd < ca.last_max_cwnd && FAST_CONVERGENCE {
            (cwnd * (BICTCP_BETA_SCALE + BETA)) / (2 * BICTCP_BETA_SCALE)
        } else {
            cwnd
        };

        max((cwnd * BETA) / BICTCP_BETA_SCALE, 2)
    }

    fn undo_cwnd(sk: &mut cong::Sock<'_, Self>) -> u32 {
        pr_info!(
            // TODO: remove
            "undo_cwnd: time {}us, start {}us",
            time::ktime_get_boot_fast_us32(),
            sk.inet_csk_ca().hystart_state.start_time
        );

        cong::reno::undo_cwnd(sk)
    }

    fn cong_avoid(sk: &mut cong::Sock<'_, Self>, _ack: u32, mut acked: u32) {
        if !sk.tcp_is_cwnd_limited() {
            return;
        }

        let tp = sk.tcp_sk_mut();

        if tp.in_slow_start() {
            acked = tp.slow_start(acked);
            if acked == 0 {
                pr_info!(
                    // TODO: remove
                    "cong_avoid: new cwnd {}, time {}us, ssthresh {}, start {}us, ss 1",
                    sk.tcp_sk().snd_cwnd(),
                    time::ktime_get_boot_fast_us32(),
                    sk.tcp_sk().snd_ssthresh(),
                    sk.inet_csk_ca().hystart_state.start_time
                );
                return;
            }
        }

        let cwnd = tp.snd_cwnd();
        let cnt = sk.inet_csk_ca_mut().update(cwnd, acked);
        sk.tcp_sk_mut().cong_avoid_ai(cnt, acked);

        pr_info!(
            // TODO: remove
            "cong_avoid: new cwnd {}, time {}us, ssthresh {}, start {}us, ss 0",
            sk.tcp_sk().snd_cwnd(),
            time::ktime_get_boot_fast_us32(),
            sk.tcp_sk().snd_ssthresh(),
            sk.inet_csk_ca().hystart_state.start_time
        );
    }
}

#[allow(non_snake_case)]
struct CubicState {
    /// Increase cwnd by one step after `cnt` ACKs.
    cnt: NonZeroU32,
    /// W__last_max.
    last_max_cwnd: u32,
    /// Value of cwnd before it was updated the last time.
    last_cwnd: u32,
    /// Time when `last_cwnd` was updated.
    last_time: time::Msecs32,
    /// Value of cwnd where the plateau of the cubic function is located.
    origin_point: u32,
    /// Time it takes to reach `origin_point`, measured from the beginning of
    /// an epoch.
    K: time::Msecs32,
    /// Time when the current epoch has started. `None` when not in congestion
    /// avoidance.
    epoch_start: Option<time::Msecs32>,
    /// Number of packets that have been ACKed in the current epoch.
    ack_cnt: u32,
    /// Estimate for the cwnd of TCP Reno.
    tcp_cwnd: u32,
    /// State of the HyStart slow start algorithm.
    hystart_state: hystart::HyStartState,
}

impl hystart::HasHyStartState for CubicState {
    fn hy(&self) -> &hystart::HyStartState {
        &self.hystart_state
    }

    fn hy_mut(&mut self) -> &mut hystart::HyStartState {
        &mut self.hystart_state
    }
}

impl Default for CubicState {
    fn default() -> Self {
        Self {
            // NOTE: Initializing this to 1 deviates from the C code. It does
            // not change the behavior.
            cnt: NonZeroU32::MIN,
            last_max_cwnd: 0,
            last_cwnd: 0,
            last_time: 0,
            origin_point: 0,
            K: 0,
            epoch_start: None,
            ack_cnt: 0,
            tcp_cwnd: 0,
            hystart_state: hystart::HyStartState::default(),
        }
    }
}

impl CubicState {
    /// Checks if the current CUBIC increase is less aggressive than normal TCP,
    /// i.e., if we are in the TCP-friendly region. If so, returns `cnt` that
    /// increases at the speed of normal TCP.
    #[inline]
    fn tcp_friendliness(&mut self, cnt: u32, cwnd: u32) -> u32 {
        if !TCP_FRIENDLINESS {
            return cnt;
        }

        // Estimate cwnd of normal TCP.
        // cwnd/3 * (1 + BETA)/(1 - BETA)
        let delta = (cwnd * BETA_SCALE) >> 3;
        // W__tcp(t) = W__tcp(t__0) + (acks(t) - acks(t__0)) / delta
        while self.ack_cnt > delta {
            self.ack_cnt -= delta;
            self.tcp_cwnd += 1;
        }

        //TODO: remove
        pr_info!(
            "tcp_friendliness: tcp_cwnd {}, cwnd {}, start {}us",
            self.tcp_cwnd,
            cwnd,
            self.hystart_state.start_time,
        );

        // We are slower than normal TCP.
        if self.tcp_cwnd > cwnd {
            let delta = self.tcp_cwnd - cwnd;

            min(cnt, cwnd / delta)
        } else {
            cnt
        }
    }

    /// Returns the new value of `cnt` to keep the window grow on the cubic
    /// curve.
    fn update(&mut self, cwnd: u32, acked: u32) -> NonZeroU32 {
        let now: time::Msecs32 = time::ktime_get_boot_fast_ms32();

        self.ack_cnt += acked;

        if self.last_cwnd == cwnd && now.wrapping_sub(self.last_time) <= time::MSEC_PER_SEC / 32 {
            return self.cnt;
        }

        // We can update the CUBIC function at most once every ms.
        if self.epoch_start.is_some() && now == self.last_time {
            let cnt = self.tcp_friendliness(self.cnt.get(), cwnd);

            // SAFETY: 2 != 0. QED.
            self.cnt = unsafe { NonZeroU32::new_unchecked(max(2, cnt)) };

            return self.cnt;
        }

        self.last_cwnd = cwnd;
        self.last_time = now;

        if self.epoch_start.is_none() {
            self.epoch_start = Some(now);
            self.ack_cnt = acked;
            self.tcp_cwnd = cwnd;

            if self.last_max_cwnd <= cwnd {
                self.K = 0;
                self.origin_point = cwnd;
            } else {
                // K = (SRTT/C * (W__max - cwnd))^1/3
                self.K = cubic_root(CUBE_FACTOR * ((self.last_max_cwnd - cwnd) as u64));
                self.origin_point = self.last_max_cwnd;
            }
        }

        // PANIC: This is always `Some`.
        let epoch_start: time::Msecs32 = self.epoch_start.unwrap();
        let Some(delay_min) = self.hystart_state.delay_min else {
            pr_err!("update: delay_min was None");
            return self.cnt;
        };

        // NOTE: Addition might overflow after 50 days without a loss, C uses a
        // `u64` here.
        let t: time::Msecs32 =
            now.wrapping_sub(epoch_start) + (delay_min / (time::USEC_PER_MSEC as time::Usecs32));
        let offs: time::Msecs32 = if t < self.K { self.K - t } else { t - self.K };

        // Calculate c/rtt * (t-K)^3 and change units to seconds.
        // Widen type to prevent overflow.
        let offs = offs as u64;
        let delta = (((CUBE_RTT_SCALE as u64 * offs * offs * offs) >> 10) / 1_000_000_000) as u32;
        // Calculate the full cubic function c/rtt * (t - K)^3 + W__max.
        let target = if t < self.K {
            self.origin_point - delta
        } else {
            self.origin_point + delta
        };

        // TODO: remove
        pr_info!(
            "update: now {}ms, epoch_start {}ms, t {}ms, K {}ms, |t - K| {}ms, last_max_cwnd {}, origin_point {}, target {}, start {}us",
            now,
            epoch_start,
            t,
            self.K,
            offs,
            self.last_max_cwnd,
            self.origin_point,
            target,
            self.hystart_state.start_time,
        );

        let mut cnt = if target > cwnd {
            cwnd / (target - cwnd)
        } else {
            // Effectively keeps cwnd constant for the next RTT.
            100 * cwnd
        };

        // In initial epoch or after timeout we grow at a minimum rate.
        if self.last_max_cwnd == 0 {
            cnt = min(cnt, 20);
        }

        // SAFETY: 2 != 0. QED.
        self.cnt = unsafe { NonZeroU32::new_unchecked(max(2, self.tcp_friendliness(cnt, cwnd))) };

        self.cnt
    }

    fn reset(&mut self) {
        // TODO: remove
        let tmp = self.hystart_state.start_time;

        *self = Self::default();

        // TODO: remove
        self.hystart_state.start_time = tmp;
    }
}

/// Calculate the cubic root of `a` using a table lookup followed by one
/// Newton-Raphson iteration.
// E[ |(cubic_root(x) - x.cbrt()) / x.cbrt()| ] = 0.71% for x in 1..1_000_000.
// E[ |(cubic_root(x) - x.cbrt()) / x.cbrt()| ] = 8.87% for x in 1..63.
// Where everything is `f64` and `.cbrt` is Rust's builtin. No overflow panics
// in this domain.
const fn cubic_root(a: u64) -> u32 {
    const V: [u8; 64] = [
        0, 54, 54, 54, 118, 118, 118, 118, 123, 129, 134, 138, 143, 147, 151, 156, 157, 161, 164,
        168, 170, 173, 176, 179, 181, 185, 187, 190, 192, 194, 197, 199, 200, 202, 204, 206, 209,
        211, 213, 215, 217, 219, 221, 222, 224, 225, 227, 229, 231, 232, 234, 236, 237, 239, 240,
        242, 244, 245, 246, 248, 250, 251, 252, 254,
    ];

    let mut b = fls64(a) as u32;
    if b < 7 {
        return ((V[a as usize] as u32) + 35) >> 6;
    }

    b = ((b * 84) >> 8) - 1;
    let shift = a >> (b * 3);

    let mut x = (((V[shift as usize] as u32) + 10) << b) >> 6;
    x = 2 * x + (a / ((x * (x - 1)) as u64)) as u32;

    (x * 341) >> 10
}

/// Find last set bit in a 64-bit word.
///
/// The last (most significant) bit is at position 64.
#[inline]
const fn fls64(x: u64) -> u8 {
    (64 - x.leading_zeros()) as u8
}
