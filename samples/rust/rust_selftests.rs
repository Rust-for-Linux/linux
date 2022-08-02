// SPDX-License-Identifier: GPL-2.0

//! Self test cases for Rust.

use kernel::prelude::*;
// Keep the `use` for a test in its test function. Module-level `use`s are only for the test
// framework.

module! {
    type: RustSelftests,
    name: "rust_selftests",
    author: "Rust for Linux Contributors",
    description: "Self test cases for Rust",
    license: "GPL",
}

struct RustSelftests;

/// A summary of testing.
///
/// A test can
///
/// * pass (successfully), or
/// * fail (without hitting any error), or
/// * hit an error (interrupted).
///
/// This is the type that differentiates the first two (pass and fail) cases.
///
/// When a test hits an error, the test function should skip and return the error. Note that this
/// doesn't mean the test fails, for example if the system doesn't have enough memory for
/// testing, the test function may return an `Err(ENOMEM)` and skip.
#[allow(dead_code)]
enum TestSummary {
    Pass,
    Fail,
}

use TestSummary::Fail;
use TestSummary::Pass;

macro_rules! do_tests {
    ($($name:ident),*) => {
        let mut total = 0;
        let mut pass = 0;
        let mut fail = 0;

        $({
            total += 1;

            match $name() {
                Ok(Pass) => {
                    pass += 1;
                    pr_info!("{} passed!", stringify!($name));
                },
                Ok(Fail) => {
                    fail += 1;
                    pr_info!("{} failed!", stringify!($name));
                },
                Err(err) => {
                    pr_info!("{} hit error {:?}", stringify!($name), err);
                }
            }
        })*

        pr_info!("{} tests run, {} passed, {} failed, {} hit errors\n",
                 total, pass, fail, total - pass - fail);

        if total == pass {
            pr_info!("All tests passed. Congratulations!\n");
        }
    }
}

fn test_rust_smp_cpu() -> Result<TestSummary> {
    use kernel::cpu::Cpu;

    if Cpu::num_possible() == 1 {
        // Nothing more to do on a single-CPU system.
        pr_info!("Skipping SMP CPU test on single-CPU system\n");
        return Ok(Pass);
    }

    let closure = || {
        let guard = Cpu::lock_current();
        let id = guard.id();
        pr_info!("Running closure on current locked processor #{id}\n");
        id
    };

    let local = Cpu::lock_current();
    pr_info!("Running on CPU #{}\n", local.id());
    let local_id = local.call(closure)?;
    if local.id() != local_id {
        pr_err!(
            "Closure did not run locally; expected={} got={}\n",
            local.id(),
            local_id
        );
        return Ok(Fail);
    }

    let remote = Cpu::from_id(if local.id() == 0 { 1 } else { 0 })?;
    let remote_id = remote.call(closure)?;
    if remote.id() != remote_id {
        pr_err!(
            "Closure did not run on expected remote cpu; expected={} got={}\n",
            remote.id(),
            remote_id
        );
        return Ok(Fail);
    }
    Ok(Pass)
}

impl kernel::Module for RustSelftests {
    fn init(_name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust self tests (init)\n");

        do_tests! {
            test_rust_smp_cpu
        };

        Ok(RustSelftests)
    }
}

impl Drop for RustSelftests {
    fn drop(&mut self) {
        pr_info!("Rust self tests (exit)\n");
    }
}
