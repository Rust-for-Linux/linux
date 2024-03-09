// SPDX-License-Identifier: GPL-2.0

//! Binary Increase Congestion control (BIC). Based on:
//!     Binary Increase Congestion Control (BIC) for Fast Long-Distance
//!     Networks - Lisong Xu, Khaled Harfoush, and Injong Rhee
//!     IEEE INFOCOM 2004, Hong Kong, China, 2004, pp. 2514-2524 vol.4
//!     doi: 10.1109/INFCOM.2004.1354672
//!     Link: https://doi.org/10.1109/INFCOM.2004.1354672
//!     Link: https://web.archive.org/web/20160417213452/http://netsrv.csc.ncsu.edu/export/bitcp.pdf

use core::cmp::{max, min};
use core::num::NonZeroU32;
use kernel::c_str;
use kernel::net::tcp::cong::{self, module_cca};
use kernel::prelude::*;
use kernel::time;

const ACK_RATIO_SHIFT: u32 = 4;

// TODO: Convert to module parameters once they are available.
/// The initial value of ssthresh for new connections. Setting this to `None`
/// implies `i32::MAX`.
const INITIAL_SSTHRESH: Option<u32> = None;
/// If cwnd is larger than this threshold, BIC engages; otherwise normal TCP
/// increase/decrease will be performed.
const LOW_WINDOW: u32 = 14;
/// In binary search, go to point: `cwnd + (W_max - cwnd) / BICTCP_B`.
// TODO: Convert to `new::(x).unwrap()` once `const_option` is stabilised.
// SAFETY: This will panic at compile time when passing zero.
const BICTCP_B: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(4) };
/// The maximum increment, i.e., `S_max`. This is used during additive increase.
/// After crossing `W_max`, slow start is performed until passing
/// `MAX_INCREMENT * (BICTCP_B - 1)`.
// TODO: Convert to `new::(x).unwrap()` once `const_option` is stabilised.
// SAFETY: This will panic at compile time when passing zero.
const MAX_INCREMENT: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(16) };
/// The number of RTT it takes to get from `W_max - BICTCP_B` to `W_max` (and
/// from `W_max` to `W_max + BICTCP_B`). This is not part of the original paper
/// and results in a slow additive increase across `W_max`.
const SMOOTH_PART: u32 = 20;
/// Whether to use fast convergence. This is a heuristic to increase the
/// release of bandwidth by existing flows to speed up the convergence to a
/// steady state when a new flow joins the link.
const FAST_CONVERGENCE: bool = true;
/// Factor for multiplicative decrease. In fast retransmit we have:
/// `cwnd = cwnd * BETA/BETA_SCALE`
/// and if fast convergence is active:
/// `W_max = cwnd * (1 + BETA/BETA_SCALE)/2`
/// instead of `W_max = cwnd`.
const BETA: u32 = 819;
/// Used to calculate beta in [0, 1] with integer arithmetics.
// TODO: Convert to `new::(x).unwrap()` once `const_option` is stabilised.
// SAFETY: This will panic at compile time when passing zero.
const BETA_SCALE: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(1024) };
/// The minimum amount of time that has to pass between two updates of the cwnd.
const MIN_UPDATE_INTERVAL: time::Msecs32 = time::MSEC_PER_SEC / 32;

module_cca! {
    type: Bic,
    name: "tcp_bic_rust",
    author: "Rust for Linux Contributors",
    description: "Binary Increase Congestion control (BIC) algorithm, Rust implementation",
    license: "GPL v2",
}

struct Bic {}

#[vtable]
impl cong::Algorithm for Bic {
    type Data = BicState;

    const NAME: &'static CStr = c_str!("bic_rust");

    fn pkts_acked(sk: &mut cong::Sock<'_, Self>, sample: &cong::AckSample) {
        if let Ok(cong::State::Open) = sk.inet_csk().ca_state() {
            let ca = sk.inet_csk_ca_mut();

            // Track delayed acknowledgment ratio using sliding window:
            // ratio = (15*ratio + sample) / 16
            ca.delayed_ack = ca.delayed_ack.wrapping_add(
                sample
                    .pkts_acked()
                    .wrapping_sub(ca.delayed_ack >> ACK_RATIO_SHIFT),
            );
        }
    }

    fn ssthresh(sk: &mut cong::Sock<'_, Self>) -> u32 {
        let cwnd = sk.tcp_sk().snd_cwnd();
        let ca = sk.inet_csk_ca_mut();

        pr_info!(
            // TODO: remove
            "Enter fast retransmit: time {}, start {}",
            time::ktime_get_boot_fast_ns(),
            ca.start_time
        );

        // Epoch has ended.
        ca.epoch_start = 0;
        ca.last_max_cwnd = if cwnd < ca.last_max_cwnd && FAST_CONVERGENCE {
            (cwnd * (BETA_SCALE.get() + BETA)) / (2 * BETA_SCALE.get())
        } else {
            cwnd
        };

        if cwnd <= LOW_WINDOW {
            // Act like normal TCP.
            max(cwnd >> 1, 2)
        } else {
            max((cwnd * BETA) / BETA_SCALE, 2)
        }
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
                    "New cwnd {}, time {}, ssthresh {}, start {}, ss 1",
                    sk.tcp_sk().snd_cwnd(),
                    time::ktime_get_boot_fast_ns(),
                    sk.tcp_sk().snd_ssthresh(),
                    sk.inet_csk_ca().start_time
                );
                return;
            }
        }

        let cwnd = tp.snd_cwnd();
        let cnt = sk.inet_csk_ca_mut().update(cwnd);
        sk.tcp_sk_mut().cong_avoid_ai(cnt, acked);

        pr_info!(
            // TODO: remove
            "New cwnd {}, time {}, ssthresh {}, start {}, ss 0",
            sk.tcp_sk().snd_cwnd(),
            time::ktime_get_boot_fast_ns(),
            sk.tcp_sk().snd_ssthresh(),
            sk.inet_csk_ca().start_time
        );
    }

    fn set_state(sk: &mut cong::Sock<'_, Self>, new_state: cong::State) {
        if matches!(new_state, cong::State::Loss) {
            pr_info!(
                // TODO: remove
                "Retransmission timeout fired: time {}, start {}",
                time::ktime_get_boot_fast_ns(),
                sk.inet_csk_ca().start_time
            );
            sk.inet_csk_ca_mut().reset()
        }
    }

    fn undo_cwnd(sk: &mut cong::Sock<'_, Self>) -> u32 {
        pr_info!(
            // TODO: remove
            "Undo cwnd reduction: time {}, start {}",
            time::ktime_get_boot_fast_ns(),
            sk.inet_csk_ca().start_time
        );

        cong::reno::undo_cwnd(sk)
    }

    fn init(sk: &mut cong::Sock<'_, Self>) {
        if let Some(ssthresh) = INITIAL_SSTHRESH {
            sk.tcp_sk_mut().set_snd_ssthresh(ssthresh);
        }

        // TODO: remove
        pr_info!("Socket created: start {}", sk.inet_csk_ca().start_time);
    }

    // TODO: remove
    fn release(sk: &mut cong::Sock<'_, Self>) {
        pr_info!(
            "Socket destroyed: start {}, end {}",
            sk.inet_csk_ca().start_time,
            time::ktime_get_boot_fast_ns()
        );
    }
}

/// Internal state of each instance of the algorithm.
struct BicState {
    /// During congestion avoidance, cwnd is increased at most every `cnt`
    /// acknowledged packets, i.e., the average increase per acknowledged packet
    /// is proportional to `1 / cnt`.
    // NOTE: The C impl initialises this to zero. It then ensures that zero is
    // never passed to `cong_avoid_ai`, which could divide by it. Make it
    // explicit in the types that zero is not a valid value.
    cnt: NonZeroU32,
    /// Last maximum `snd_cwnd`, i.e, `W_max`.
    last_max_cwnd: u32,
    /// The last `snd_cwnd`.
    last_cwnd: u32,
    /// Time when `last_cwnd` was updated.
    last_time: time::Msecs32,
    /// Records the beginning of an epoch.
    epoch_start: time::Msecs32,
    /// Estimates the ratio of `packets/ACK << 4`. This allows us to adjust cwnd
    /// per packet when a receiver is sending a single ACK for multiple received
    /// packets.
    delayed_ack: u32,
    /// Time when algorithm was initialised.
    // TODO: remove
    start_time: time::Nsecs,
}

impl Default for BicState {
    fn default() -> Self {
        Self {
            // NOTE: Initialising this to 1 deviates from the C code. It does
            // not change the behaviour of the algorithm.
            cnt: NonZeroU32::MIN,
            last_max_cwnd: 0,
            last_cwnd: 0,
            last_time: 0,
            epoch_start: 0,
            delayed_ack: 2 << ACK_RATIO_SHIFT,
            // TODO: remove
            start_time: time::ktime_get_boot_fast_ns(),
        }
    }
}

impl BicState {
    /// Compute congestion window to use. Returns the new `cnt`.
    ///
    /// This governs the behavior of the algorithm during congestion avoidance.
    fn update(&mut self, cwnd: u32) -> NonZeroU32 {
        let now = time::ktime_get_boot_fast_ms32();

        // Do nothing if we are invoked too frequently.
        if self.last_cwnd == cwnd && now.wrapping_sub(self.last_time) <= MIN_UPDATE_INTERVAL {
            return self.cnt;
        }

        self.last_cwnd = cwnd;
        self.last_time = now;

        // Record the beginning of an epoch.
        if self.epoch_start == 0 {
            self.epoch_start = now;
        }

        // Start off like normal TCP.
        if cwnd <= LOW_WINDOW {
            self.cnt = NonZeroU32::new(cwnd).unwrap_or(NonZeroU32::MIN);
            return self.cnt;
        }

        let mut new_cnt = if cwnd < self.last_max_cwnd {
            // binary increase
            let dist: u32 = (self.last_max_cwnd - cwnd) / BICTCP_B;

            if dist > MAX_INCREMENT.get() {
                // additive increase
                cwnd / MAX_INCREMENT
            } else if dist <= 1 {
                // careful additive increase
                (cwnd * SMOOTH_PART) / BICTCP_B
            } else {
                // binary search
                cwnd / dist
            }
        } else {
            if cwnd < self.last_max_cwnd + BICTCP_B.get() {
                // careful additive increase
                (cwnd * SMOOTH_PART) / BICTCP_B
            } else if cwnd < self.last_max_cwnd + MAX_INCREMENT.get() * (BICTCP_B.get() - 1) {
                // slow start
                (cwnd * (BICTCP_B.get() - 1)) / (cwnd - self.last_max_cwnd)
            } else {
                // linear increase
                cwnd / MAX_INCREMENT
            }
        };

        // If in initial slow start or link utilization is very low.
        if self.last_max_cwnd == 0 {
            new_cnt = min(new_cnt, 20);
        }

        // Account for estimated packets/ACK to ensure that we increase per
        // packet.
        new_cnt = (new_cnt << ACK_RATIO_SHIFT) / self.delayed_ack;

        self.cnt = NonZeroU32::new(new_cnt).unwrap_or(NonZeroU32::MIN);

        self.cnt
    }

    fn reset(&mut self) {
        // TODO: remove
        let tmp = self.start_time;

        *self = Self::default();

        // TODO: remove
        self.start_time = tmp;
    }
}
