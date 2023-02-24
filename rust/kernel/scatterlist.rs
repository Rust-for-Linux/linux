// SPDX-License-Identifier: GPL-2.0

//! scatterlist
//!
//! C header: [`include/scatterlist.h`](../../../../../include/scatterlist.h)

#![allow(dead_code)]

use crate::bindings;

/// A `Scatterlist` represents an area of physical memory.
///
/// `Scatterlist` is always shown as an array. Each `Scatterlist` represents a continuous memory area in a page.
/// The `Scatterlist` array can represent continuous physical memory area.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Scatterlist<const L: usize> {
    pub(crate) scatterlist: bindings::scatterlist,
}

impl<const L: usize> Scatterlist<L> {
    /// New an array of `Scatterlist` and initialize the array.
    pub fn new_and_init_sg_table() -> [Self; L] {
        let mut sgs = [Self {
            scatterlist: bindings::scatterlist::default(),
        }; L];
        // SAFETY: The address of `sgs` is valid and the `L` matches the length of `sgs`.
        unsafe {
            bindings::sg_init_table(
                &mut sgs as *mut [Self; L] as *mut bindings::scatterlist,
                L.try_into().unwrap(),
            );
        }
        sgs
    }
}
