// SPDX-License-Identifier: GPL-2.0

//! Rust synchronisation primitives sample.

use kernel::prelude::*;
use kernel::{
    condvar_init, mutex_init, spinlock_init,
    sync::{CondVar, Mutex, SpinLock},
};

module! {
    type: RustSync,
    name: "rust_sync",
    authors: ["Rust for Linux Contributors"],
    description: "Rust synchronisation primitives sample",
    license: "GPL",
}

kernel::init_static_sync! {
    static SAMPLE_MUTEX: Mutex<u32> = 10;
    static SAMPLE_CONDVAR: CondVar;
}

struct RustSync;

impl kernel::Module for RustSync {
    fn init(_name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust synchronisation primitives sample (init)\n");

        // Test mutexes.
        {
            // SAFETY: `init` is called below.
            let mut data = Pin::from(Box::try_new(unsafe { Mutex::new(0) })?);
            mutex_init!(data.as_mut(), "RustSync::init::data1");
            *data.lock() = 10;
            pr_info!("Value: {}\n", *data.lock());

            // SAFETY: `init` is called below.
            let mut cv = Pin::from(Box::try_new(unsafe { CondVar::new() })?);
            condvar_init!(cv.as_mut(), "RustSync::init::cv1");

            {
                let mut guard = data.lock();
                while *guard != 10 {
                    let _ = cv.wait(&mut guard);
                }
            }
            cv.notify_one();
            cv.notify_all();
            cv.free_waiters();
        }

        // Test static mutex + condvar.
        *SAMPLE_MUTEX.lock() = 20;

        {
            let mut guard = SAMPLE_MUTEX.lock();
            while *guard != 20 {
                let _ = SAMPLE_CONDVAR.wait(&mut guard);
            }
        }

        // Test spinlocks.
        {
            // SAFETY: `init` is called below.
            let mut data = Pin::from(Box::try_new(unsafe { SpinLock::new(0) })?);
            spinlock_init!(data.as_mut(), "RustSync::init::data2");
            *data.lock() = 10;
            pr_info!("Value: {}\n", *data.lock());

            // SAFETY: `init` is called below.
            let mut cv = Pin::from(Box::try_new(unsafe { CondVar::new() })?);
            condvar_init!(cv.as_mut(), "RustSync::init::cv2");
            {
                let mut guard = data.lock();
                while *guard != 10 {
                    let _ = cv.wait(&mut guard);
                }
            }
            cv.notify_one();
            cv.notify_all();
            cv.free_waiters();
        }

        Ok(RustSync)
    }
}

impl Drop for RustSync {
    fn drop(&mut self) {
        pr_info!("Rust synchronisation primitives sample (exit)\n");
    }
}
