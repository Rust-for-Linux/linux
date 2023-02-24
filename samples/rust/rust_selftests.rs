// SPDX-License-Identifier: GPL-2.0

//! Self test cases for Rust.

use kernel::prelude::*;
// Keep the `use` for a test in its test function. Module-level `use`s are only for the test
// framework.

module! {
    type: RustSelftests,
    name: "rust_selftests",
    authors: ["Rust for Linux Contributors"],
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

/// An example of test.
#[allow(dead_code)]
fn test_example() -> Result<TestSummary> {
    // `use` declarations for the test can be put here, e.g. `use foo::bar;`.

    // Always pass.
    Ok(Pass)
}

impl kernel::Module for RustSelftests {
    fn init(_name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust self tests (init)\n");

        do_tests! {
            test_example // TODO: Remove when there is at least a real test.
        };

        Ok(RustSelftests)
    }
}

impl Drop for RustSelftests {
    fn drop(&mut self) {
        pr_info!("Rust self tests (exit)\n");
    }
}
