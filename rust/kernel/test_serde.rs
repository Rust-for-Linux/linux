// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Test `serde`.
//!
//! It contains a data format used by the `rust_serde` sample, as well
//! as a quick check that `serde_derive` works in the `kernel` crate too.

#![allow(missing_docs)]

mod de;
mod error;
mod ser;

pub use de::{from_bytes, Deserializer};
pub use error::{Error, Result};
pub use ser::{to_vec, Serializer};

use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct S {
    a: (),
    b: bool,
    c: bool,
    d: (),
}
