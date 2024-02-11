//! Congestion control algorithm example.
use core::num::NonZeroU32;
use kernel::net::tcp::cong::*;
use kernel::prelude::*;
use kernel::{c_str, module_cca};

struct MyCca {}

#[vtable]
impl Algorithm for MyCca {
    type Data = ();

    const NAME: &'static CStr = c_str!("my_cca");

    fn undo_cwnd(sk: &mut Sock<'_, Self>) -> u32 {
        reno::undo_cwnd(sk)
    }

    fn ssthresh(_sk: &mut Sock<'_, Self>) -> u32 {
        2
    }

    fn cong_avoid(sk: &mut Sock<'_, Self>, _ack: u32, acked: u32) {
        sk.tcp_sk_mut()
            .cong_avoid_ai(NonZeroU32::new(1).unwrap(), acked)
    }
}

module_cca! {
    type: MyCca,
    name: "my_cca",
    author: "Rust for Linux Contributors",
    description: "Sample congestion control algorithm implemented in Rust.",
    license: "GPL v2",
}
