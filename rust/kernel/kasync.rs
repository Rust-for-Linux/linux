// SPDX-License-Identifier: GPL-2.0

//! Kernel async functionality.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub mod executor;
#[cfg(CONFIG_NET)]
pub mod net;

/// Yields execution of the current task so that other tasks may execute.
///
/// The task continues to be in a "runnable" state though, so it will eventually run again.
///
/// # Examples
///
/// ```
/// use kernel::kasync::yield_now;
///
/// async fn example() {
///     pr_info!("Before yield\n");
///     yield_now().await;
///     pr_info!("After yield\n");
/// }
/// ```
pub fn yield_now() -> impl Future<Output = ()> {
    struct Yield {
        first_poll: bool,
    }

    impl Future for Yield {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if !self.first_poll {
                Poll::Ready(())
            } else {
                self.first_poll = false;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    Yield { first_poll: true }
}
