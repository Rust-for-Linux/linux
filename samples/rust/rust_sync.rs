// SPDX-License-Identifier: GPL-2.0

//! Rust synchronisation primitives sample.

use kernel::prelude::*;
use kernel::{
    new_condvar, new_mutex, new_spinlock,
    sync::{CondVar, Mutex},
};

module! {
    type: RustSync,
    name: "rust_sync",
    author: "Rust for Linux Contributors",
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
            let data = Box::pin_init(new_mutex!(0, "RustSync::init::data1"))?;
            *data.lock() = 10;
            pr_info!("Value: {}\n", *data.lock());

            let cv = Box::pin_init(new_condvar!("RustSync::init::cv1"))?;
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
            let data = Box::pin_init(new_spinlock!(0, "RustSync::init::data2"))?;
            *data.lock() = 10;
            pr_info!("Value: {}\n", *data.lock());

            let cv = Box::pin_init(new_condvar!("RustSync::init::cv2"))?;
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
