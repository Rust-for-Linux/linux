// SPDX-License-Identifier: GPL-2.0

//! Rust minimal in-place sample.

use kernel::prelude::*;

module! {
    type: RustInPlace,
    name: "rust_inplace",
    author: "Rust for Linux Contributors",
    description: "Rust minimal in-place sample",
    license: "GPL",
}

#[pin_data(PinnedDrop)]
struct RustInPlace {
    numbers: Vec<i32>,
}

impl kernel::InPlaceModule for RustInPlace {
    fn init(_module: &'static ThisModule) -> impl PinInit<Self, Error> {
        pr_info!("Rust minimal sample (init)\n");
        pr_info!("Am I built-in? {}\n", !cfg!(MODULE));
        try_pin_init!(Self {
            numbers: {
                let mut numbers = Vec::new();
                numbers.try_push(72)?;
                numbers.try_push(108)?;
                numbers.try_push(200)?;
                numbers
            },
        })
    }
}

#[pinned_drop]
impl PinnedDrop for RustInPlace {
    fn drop(self: Pin<&mut Self>) {
        pr_info!("My numbers are {:?}\n", self.numbers);
        pr_info!("Rust minimal inplace sample (exit)\n");
    }
}
