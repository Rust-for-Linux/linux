// SPDX-License-Identifier: GPL-2.0

//! Rust `serde` sample.
//!
//! It uses a data format from the `kernel` crate, as well as defining
//! one here ("local"). Then it uses both on a type that uses `serve_derive`.

use kernel::prelude::*;
use serde_derive::{Deserialize, Serialize};

module! {
    type: RustSerde,
    name: "rust_serde",
    author: "Rust for Linux Contributors",
    description: "Rust `serde` sample",
    license: "GPL",
}

struct RustSerde;

pub mod local_data_format {
    #![allow(missing_docs)]

    mod de;
    mod error;
    mod ser;

    pub use de::{from_bytes, Deserializer};
    pub use error::{Error, Result};
    pub use ser::{to_vec, Serializer};
}

#[derive(Serialize, Deserialize, Debug)]
struct S {
    a: (),
    b: bool,
    c: bool,
    d: (),
}

impl kernel::Module for RustSerde {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust serde sample (init)\n");

        let original = S {
            a: (),
            b: false,
            c: true,
            d: (),
        };
        crate::pr_info!("            original = {:?}", original);

        let serialized = kernel::test_serde::to_vec(&original).unwrap();
        crate::pr_info!("          serialized = {:?}", serialized);

        let deserialized: S = kernel::test_serde::from_bytes(&serialized).unwrap();
        crate::pr_info!("        deserialized = {:?}", deserialized);

        let serialized = local_data_format::to_vec(&deserialized).unwrap();
        crate::pr_info!("  serialized (local) = {:?}", serialized);

        let deserialized: S = local_data_format::from_bytes(&serialized).unwrap();
        crate::pr_info!("deserialized (local) = {:?}", deserialized);

        Ok(RustSerde)
    }
}

impl Drop for RustSerde {
    fn drop(&mut self) {
        pr_info!("Rust serde sample (exit)\n");
    }
}
