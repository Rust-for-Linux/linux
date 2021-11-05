// SPDX-License-Identifier: GPL-2.0

//! Rust printing macros sample

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::pr_cont;
use kernel::prelude::*;

module! {
    type: RustPrint,
    name: b"rust_print",
    author: b"Rust for Linux Contributors",
    description: b"Rust printing macros sample",
    license: b"GPL v2",
}

struct RustPrint;

impl KernelModule for RustPrint {
    fn init() -> Result<Self> {
        pr_info!("Rust printing macros sample (init)\n");

        pr_emerg!("Emergency message (level 0) without args\n");
        pr_alert!("Alert message (level 1) without args\n");
        pr_crit!("Critical message (level 2) without args\n");
        pr_err!("Error message (level 3) without args\n");
        pr_warn!("Warning message (level 4) without args\n");
        pr_notice!("Notice message (level 5) without args\n");
        pr_info!("Info message (level 6) without args\n");

        pr_info!("A line that");
        pr_cont!(" is continued");
        pr_cont!(" without args\n");

        pr_emerg!("{} message (level {}) with args\n", "Emergency", 0);
        pr_alert!("{} message (level {}) with args\n", "Alert", 1);
        pr_crit!("{} message (level {}) with args\n", "Critical", 2);
        pr_err!("{} message (level {}) with args\n", "Error", 3);
        pr_warn!("{} message (level {}) with args\n", "Warning", 4);
        pr_notice!("{} message (level {}) with args\n", "Notice", 5);
        pr_info!("{} message (level {}) with args\n", "Info", 6);

        pr_info!("A {} that", "line");
        pr_cont!(" is {}", "continued");
        pr_cont!(" with {}\n", "args");

        for i in 0..100 {
            pr_emerg_ratelimited!("RustPrint::init", "Repetitive Emergency message {}", i);
            pr_alert_ratelimited!("RustPrint::init", "Repetitive Alert message {}", i);
            pr_crit_ratelimited!("RustPrint::init", "Repetitive Critical message {}", i);
            pr_err_ratelimited!("RustPrint::init", "Repetitive Error message {}", i);
            pr_warn_ratelimited!("RustPrint::init", "Repetitive Warning message {}", i);
            pr_notice_ratelimited!("RustPrint::init", "Repetitive Notice message {}", i);
            pr_info_ratelimited!("RustPrint::init", "Repetitive Info message {}", i);

            if printk_ratelimit!("RustPrint::init") {
                pr_warn!("Inside printk_ratelimit {}", i);
            }
        }

        Ok(RustPrint)
    }
}

impl Drop for RustPrint {
    fn drop(&mut self) {
        pr_info!("Rust printing macros sample (exit)\n");
    }
}
