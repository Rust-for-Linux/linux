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

/// An example of test.
#[allow(dead_code)]
fn test_example() -> Result<TestSummary> {
    // `use` declarations for the test can be put here, e.g. `use foo::bar;`.

    // Always pass.
    Ok(Pass)
}

/// Alignment requirement test for allocator.
fn test_alloc_alignment() -> Result<TestSummary> {
        use alloc::alloc::{alloc, dealloc};
        use core::alloc::Layout;
        use core::ptr;

        const MAX_ALIGN_ORDER: usize = 12;

        let mut unaligned = false;
        let mut min_unaligned = [(usize::MAX, ptr::null_mut()); 13];
        let mut max_unaligned = [(0, ptr::null_mut()); 13];

        // Generates alignments [1,2,..,2^MAX_ALIGN_ORDER].
        for align in 0..=MAX_ALIGN_ORDER {
            // Generates sizes [1, 4 * alignment).
            for size in 1..(1 << (align + 2)) {
                let layout = Layout::from_size_align(size, 1 << align)?;

                // SAFETY: layout.size() is not zero.
                let ptr = unsafe { alloc(layout) };

                // Allocation failed, report out of memory.
                if ptr.is_null() {
                    return Err(ENOMEM);
                }

                if (ptr as usize) & ((1 << align) - 1) != 0 {
                    unaligned = true;

                    if min_unaligned[align].0 > size {
                        min_unaligned[align] = (size, ptr);
                    }

                    if max_unaligned[align].0 < size {
                        max_unaligned[align] = (size, ptr);
                    }
                }

                // SAFETY: Free the `ptr` allocated from the above `alloc` with the same
                // layout.
                unsafe { dealloc(ptr, layout) };
            }
        }

        if unaligned {
            pr_info!("Alignment mismatch found:");
            for align in 0..=MAX_ALIGN_ORDER {
                if max_unaligned[align].0 != 0 {
                    pr_info!(
                        " alignment = {}, min size = {} with ptr = {:?}, max size = {} with ptr = {:?}",
                        1 << align, min_unaligned[align].0, min_unaligned[align].1,
                        max_unaligned[align].0, max_unaligned[align].1);
                }
            }
            Ok(Fail)
        } else {
            Ok(Pass)
        }
}

impl kernel::Module for RustSelftests {
    fn init(_name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust self tests (init)\n");

        do_tests! {
            test_alloc_alignment
        };

        Ok(RustSelftests)
    }
}

impl Drop for RustSelftests {
    fn drop(&mut self) {
        pr_info!("Rust self tests (exit)\n");
    }
}
