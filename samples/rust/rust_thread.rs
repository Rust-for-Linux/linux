// SPDX-License-Identifier: GPL-2.0

//! Rust thread APIs sample

#![no_std]
#![feature(allocator_api, global_asm)]

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel::prelude::*;
use kernel::{
    c_str, pr_cont,
    sync::Ref,
    thread::{schedule, Thread},
};

module! {
    type: RustThread,
    name: b"rust_thread",
    author: b"Rust for Linux Contributors",
    description: b"Rust thread APIs sample",
    license: b"GPL v2",
    params: {
    },
}

struct RustThread;

impl KernelModule for RustThread {
    fn init() -> Result<Self> {
        pr_info!("Rust thread APIs sample (init)");

        // Test threads.
        {
            Thread::try_new(c_str!("rust-thread0"), || {
                pr_info!("Hello Rust Thread");
                Ok(())
            })?
            .wake_up();
        }

        // Test threads (wait for the thread to finish).
        {
            let mut a = 1;
            // Flag that tells whether the thread gets executed.
            let flag = Ref::try_new(AtomicBool::new(false))?;
            let other = flag.clone();
            let boxed_slice: Box<[i32]> = Box::try_new([1, 3, 4, 5])?;

            let t1 = Thread::try_new(c_str!("rust-thread1"), move || {
                other.store(true, Ordering::Release);
                let b = Box::try_new(42)?;
                for _ in 0..20 {
                    a += 1;
                    pr_info!("Hello Rust Thread {}", a + b.as_ref());
                }
                pr_info!("A Box<[i32]>: ");
                for i in boxed_slice.iter() {
                    pr_cont!("{} ", i);
                }
                pr_info!("");

                Ok(())
            })?;

            t1.wake_up();

            // Waits to observe the thread run.
            while !flag.load(Ordering::Acquire) {
                schedule();
            }

            // `t1` should exit normally.
            t1.stop().expect("Rust thread should exit normally");
        }

        // Test threads (not up for running).
        {
            // One-off struct only to call `drop`.
            struct ToDrop(Ref<AtomicBool>);

            impl Drop for ToDrop {
                fn drop(&mut self) {
                    // Indicate the drop happened.
                    self.0.store(true, Ordering::Relaxed);
                }
            }

            unsafe impl Send for ToDrop {}

            // Flag that tells whether the thread gets executed.
            let flag = Ref::try_new(AtomicBool::new(false))?;
            let other = flag.clone();

            // Flag that tells whether `drop` gets executed.
            let drop_flag = Ref::try_new(AtomicBool::new(false))?;
            let to_drop = ToDrop(drop_flag.clone());

            let mut a = 1;

            let t1 = Thread::try_new(c_str!("rust-thread2"), move || {
                let b = Box::try_new(42)?;
                for _ in 0..20 {
                    a += 1;
                    pr_info!("Hello Rust Thread {}", a + b.as_ref());
                }

                // moves `to_drop` in.
                let _ = to_drop;

                // sets the flag to show that the thread gets executed.
                other.store(true, Ordering::Relaxed);

                Ok(())
            })?;

            // The thread hasn't executed and the `to_drop` is not dropped.
            assert!(!(flag.load(Ordering::Relaxed)) && !(drop_flag.load(Ordering::Relaxed)));

            // Without `wake_up`, `stop` will cause the thread to exits with `-EINTR`.
            t1.stop().expect_err("Rust thread should exit abnormally");

            // The thread hasn't executed and the `to_drop` is dropped.
            assert!(!(flag.load(Ordering::Relaxed)) && drop_flag.load(Ordering::Relaxed));
        }

        Ok(RustThread)
    }
}

impl Drop for RustThread {
    fn drop(&mut self) {
        pr_info!("Rust thread APIs sample (exit)");
    }
}
