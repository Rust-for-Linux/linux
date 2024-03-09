// SPDX-License-Identifier: GPL-2.0-only

//! HyStart slow start algorithm.
//!
//! Based on:
//!     Sangtae Ha, Injong Rhee,
//!     Taming the elephants: New TCP slow start,
//!     Computer Networks, Volume 55, Issue 9, 2011, Pages 2092-2110,
//!     ISSN 1389-1286, <https://doi.org/10.1016/j.comnet.2011.01.014>

use crate::net::sock;
use crate::net::tcp::{self, cong};
use crate::time;
use crate::{pr_err, pr_info};
use core::cmp::min;

/// The heuristic that is used to find the exit point for slow start.
pub enum HystartDetect {
    /// Exits slow start when the length of so-called ACK-trains becomes equal
    /// to the estimated minimum forward path one-way delay.
    AckTrain = 1,
    /// Exits slow start when the estimated RTT increase between two consecutive
    /// rounds exceeds a threshold that is based on the last RTT.
    Delay = 2,
    /// Combine both algorithms.
    Both = 3,
}

/// Internal state of the [`HyStart`] algorithm.
pub struct HyStartState {
    /// Number of ACKs already sampled to determine the RTT of this round.
    sample_cnt: u8,
    /// Whether the slow start exit point was found.
    found: bool,
    /// Time when the current round has started.
    round_start: time::Usecs32,
    /// Sequence number of the byte that marks the end of the current round.
    end_seq: u32,
    /// Time when the last ACK was received in this round.
    last_ack: time::Usecs32,
    /// The minimum RTT of the current round.
    curr_rtt: time::Usecs32,
    /// Estimate of the minimum forward path one-way delay of the link.
    pub delay_min: Option<time::Usecs32>,
    /// Time when the connection was created.
    // TODO: remove
    pub start_time: time::Usecs32,
}

impl Default for HyStartState {
    fn default() -> Self {
        Self {
            sample_cnt: 0,
            found: false,
            round_start: 0,
            end_seq: 0,
            last_ack: 0,
            curr_rtt: 0,
            delay_min: None,
            // TODO: remove
            start_time: time::ktime_get_boot_fast_us32(),
        }
    }
}

impl HyStartState {
    /// Returns true iff the algorithm `T` is in hybrid slow start.
    #[inline]
    pub fn in_hystart<T: HyStart>(&self, cwnd: u32) -> bool {
        !self.found && cwnd >= T::LOW_WINDOW
    }
}

/// Implement this trait on [`Algorithm::Data`] to use [`HyStart`] for your CCA.
///
/// [`Algorithm::Data`]: cong::Algorithm::Data
pub trait HasHyStartState {
    /// Returns the private data of the HyStart algorithm.
    fn hy(&self) -> &HyStartState;

    /// Returns the private data of the HyStart algorithm.
    fn hy_mut(&mut self) -> &mut HyStartState;
}

/// Implement this trait on your [`Algorithm`] to use HyStart. You still need to
/// invoke the [`reset`] and [`update`] methods at the right places.
///
/// [`Algorithm`]: cong::Algorithm
/// [`reset`]: HyStart::reset
/// [`update`]: HyStart::update
pub trait HyStart: cong::Algorithm<Data: HasHyStartState> {
    // TODO: Those constants should be configurable via module parameters.
    /// Which heuristic to use for deciding when it is time to exit slow start.
    const DETECT: HystartDetect;

    /// Lower bound for cwnd during hybrid slow start.
    const LOW_WINDOW: u32;

    /// Max spacing between ACKs in an ACK-train.
    const ACK_DELTA: time::Usecs32;

    /// Number of ACKs to sample at the beginning of each round to estimate the
    /// RTT of this round.
    const MIN_SAMPLES: u8 = 8;

    /// Lower bound on the increase in RTT between to consecutive rounds that is
    /// needed to trigger an exit from slow start.
    const DELAY_MIN: time::Usecs32 = 4000;

    /// Upper bound on the increase in RTT between to consecutive rounds that is
    /// needed to trigger an exit from slow start.
    const DELAY_MAX: time::Usecs32 = 16000;

    /// Corresponds to the function eta from the paper. Returns the increase in
    /// RTT between consecutive rounds that triggers and exit from slow start.
    /// `t` is the RTT of the last round.
    fn delay_thresh(mut t: time::Usecs32) -> time::Usecs32 {
        t >>= 3;

        if t < Self::DELAY_MIN {
            Self::DELAY_MIN
        } else if t > Self::DELAY_MAX {
            Self::DELAY_MAX
        } else {
            t
        }
    }

    /// TODO
    fn ack_delay(sk: &cong::Sock<'_, Self>) -> time::Usecs32 {
        (match sk.sk_pacing_rate() {
            0 => 0,
            rate => min(
                time::USEC_PER_MSEC,
                ((sk.sk_gso_max_size() as u64) * 4 * time::USEC_PER_SEC) / rate,
            ),
        } as time::Usecs32)
    }

    /// Called in slow start at the beginning of a new round of incoming ACKs.
    fn reset(sk: &mut cong::Sock<'_, Self>) {
        let tp = sk.tcp_sk();
        let now = tp.tcp_mstamp() as time::Usecs32;
        let snd_nxt = tp.snd_nxt();

        let hy = sk.inet_csk_ca_mut().hy_mut();

        hy.round_start = now;
        hy.last_ack = now;
        hy.end_seq = snd_nxt;
        hy.curr_rtt = u32::MAX;
        hy.sample_cnt = 0;
    }

    /// Called in slow start to decide if it is time to exit slow start. Sets
    /// [`HyStartState`] `found` to true when it is time to exit.
    fn update(sk: &mut cong::Sock<'_, Self>, delay: time::Usecs32) {
        // Start of a new round.
        if tcp::after(sk.tcp_sk().snd_una(), sk.inet_csk_ca().hy().end_seq) {
            Self::reset(sk);
        }
        let hy = sk.inet_csk_ca().hy();
        let Some(delay_min) = hy.delay_min else {
            // This should not happen.
            pr_err!("hystart: update: delay_min was None");
            return;
        };

        if matches!(Self::DETECT, HystartDetect::Both | HystartDetect::AckTrain) {
            let tp = sk.tcp_sk();
            let now = tp.tcp_mstamp() as time::Usecs32;

            // Is this ACK part of a train?
            // NOTE: I don't get it. C is doing this as a signed comparison but
            // for:
            // -- `0 <= now < ca->last_ack <= 0x7F..F` this means it always
            //    passes,
            // -- `ca->last_ack = 0x80..0` and `0 <= new <= 0x7F..F` it also
            //    always passes,
            // -- `0x80..00 < ca->last_ack` and `now < 0x80.0` (big enough)
            //    also always passes.
            // If I understand the paper correctly, this is not what is
            // intended. What we really want here is the unsigned version I
            // guess, please correct me if I am wrong.
            // Commit: c54b4b7655447c1f24f6d50779c22eba9ee0fd24
            // Purposefully introduced the cast ... am I just stupid?
            // Link: https://godbolt.org/z/E7ocxae69
            if now.wrapping_sub(hy.last_ack) <= Self::ACK_DELTA {
                let threshold = if let Ok(sock::Pacing::r#None) = sk.sk_pacing_status() {
                    (delay_min + Self::ack_delay(sk)) >> 1
                } else {
                    delay_min + Self::ack_delay(sk)
                };

                // Does the length of this ACK-train indicate it is time to
                // exit slow start?
                // NOTE: C is a bit weird here ... `threshold` is unsigned but
                // the lhs is still cast to signed, even though the usual
                // arithmetic conversions will immediately cast it back to
                // unsigned; thus, I guess we can just do everything unsigned.
                if now.wrapping_sub(hy.round_start) > threshold {
                    // TODO: change to debug
                    pr_info!(
                        "hystart_ack_train ({}us > {}us) delay_min {}us (+ ack_delay {}us) cwnd {}, start {}us",
                        now.wrapping_sub(hy.round_start),
                        threshold,
                        delay_min,
                        Self::ack_delay(sk),
                        tp.snd_cwnd(),
                        hy.start_time
                    );

                    let tp = sk.tcp_sk_mut();

                    tp.set_snd_ssthresh(tp.snd_cwnd());

                    sk.inet_csk_ca_mut().hy_mut().found = true;

                    // TODO: Update net stats.
                }

                sk.inet_csk_ca_mut().hy_mut().last_ack = now;
            }
        }

        if matches!(Self::DETECT, HystartDetect::Both | HystartDetect::Delay) {
            let hy = sk.inet_csk_ca_mut().hy_mut();

            // The paper only takes the min RTT of the first `MIN_SAMPLES`
            // ACKs in a round, but it does no harm to consider later ACKs as
            // well.
            if hy.curr_rtt > delay {
                hy.curr_rtt = delay
            }

            if hy.sample_cnt < Self::MIN_SAMPLES {
                hy.sample_cnt += 1;
            } else {
                // Does the increase in RTT indicate its time to exit slow
                // start?
                if hy.curr_rtt > delay_min + Self::delay_thresh(delay_min) {
                    hy.found = true;

                    // TODO: change to debug
                    let curr_rtt = hy.curr_rtt;
                    let start_time = hy.start_time;
                    pr_info!(
                        "hystart_delay: {}us > {}us, delay_min {}us (+ delay_thresh {}us), cwnd {}, start {}us",
                        curr_rtt,
                        delay_min + Self::delay_thresh(delay_min),
                        delay_min,
                        Self::delay_thresh(delay_min),
                        sk.tcp_sk().snd_cwnd(),
                        start_time,
                    );
                    // TODO: Update net stats.

                    let tp = sk.tcp_sk_mut();

                    tp.set_snd_ssthresh(tp.snd_cwnd());
                }
            }
        }
    }
}
