// SPDX-License-Identifier: GPL-2.0
//! A newtype for physical addresses.
//!
//! This module provides `PhysAddr`, a type-safe wrapper around the kernel's
//! `phys_addr_t`. Using a newtype prevents accidentally mixing physical
//! addresses with virtual addresses or other integer types, which is a common
//! source of bugs.
//!
//! The API prioritizes explicitness and safety. Conversions to and from the
//! raw integer type must be done via `from_raw()` and `as_raw()`. Arithmetic
//! is provided through both explicit methods (`checked_add`, `wrapping_add`)
//! and ergonomic operators (`+`, `-`) that have well-defined wrapping behavior.
//!
//! # Examples
//!
//! ```
//! use kernel::bindings;
//! use kernel::phys_addr::{PhysAddr};
//!
//!
//! let addr = PhysAddr::from_raw(0x1008 as bindings::phys_addr_t);
//!
//! // The `+` operator uses wrapping arithmetic.
//! assert_eq!(addr + 8, PhysAddr::from_raw(0x1010 as bindings::phys_addr_t));
//!
//! // For safety, checked arithmetic is also available.
//! let new_addr = addr.checked_add(8).expect("Overflow occurred");
//! assert_eq!(new_addr.as_raw(), 0x1010);
//!
//! // Alignment is a common task.
//! let aligned_addr = addr.align_down(8);
//! assert_eq!(aligned_addr.as_raw(), 0x1008);
//! ```
#![allow(dead_code)]

use crate::{
    bindings,
    error::{Result},
};
use core::{
    fmt,
    ops::{Add, Sub},
};
use macros::kunit_tests;

/// A newtype wrapper for a physical address.
///
/// Its size is guaranteed to match the C kernel's `phys_addr_t` by wrapping
/// the generated binding. `#[repr(transparent)]` ensures it has an identical
/// memory layout, making it safe to use across FFI boundaries.
#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysAddr(pub bindings::phys_addr_t);

impl PhysAddr {
    /// The zero physical address. Equivalent to `PhysAddr::default()`.
    pub const ZERO: Self = Self(0);

    /// Creates a `PhysAddr` from a raw `phys_addr_t` value.
    pub fn from_raw(addr: bindings::phys_addr_t) -> Self {
        Self(addr)
    }

    /// Returns the raw `phys_addr_t` value of the physical address.
    pub fn as_raw(self) -> bindings::phys_addr_t {
        self.0
    }

    /// Returns `true` if the physical address is null (zero).
    pub fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Checked addition. Returns `None` if overflow occurs.
    pub fn checked_add(self, rhs: bindings::phys_addr_t) -> Option<Self> {
        self.0.checked_add(rhs).map(Self)
    }

    /// Wrapping addition.
    pub fn wrapping_add(self, rhs: bindings::phys_addr_t) -> Self {
        Self(self.0.wrapping_add(rhs))
    }

    /// Wrapping subtraction.
    pub fn wrapping_sub(self, rhs: bindings::phys_addr_t) -> Self {
        Self(self.0.wrapping_sub(rhs))
    }

    /// Aligns the address down to the nearest multiple of `align`.
    ///
    /// `align` must be a power of two.
    pub fn align_down(self, align: bindings::phys_addr_t) -> Self {
        Self(self.0 & !(align.wrapping_sub(1)))
    }

    /// Aligns the address up to the nearest multiple of `align`.
    ///
    /// `align` must be a power of two.
    pub fn align_up(self, align: bindings::phys_addr_t) -> Self {
        // This is the idiomatic way to perform align_up.
        self.add(align.wrapping_sub(1)).align_down(align)
   }
}

// Implement operators for ergonomic pointer arithmetic.
// The convention is that these operators perform wrapping arithmetic.
impl Add<bindings::phys_addr_t> for PhysAddr {
    type Output = Self;

    fn add(self, rhs: bindings::phys_addr_t) -> Self::Output {
        self.wrapping_add(rhs)
    }
}

impl Sub<bindings::phys_addr_t> for PhysAddr {
    type Output = Self;

    fn sub(self, rhs: bindings::phys_addr_t) -> Self::Output {
        self.wrapping_sub(rhs)
    }
}

// Find the offset between two addresses.
impl Sub<PhysAddr> for PhysAddr {
    type Output = bindings::phys_addr_t;

    /// Calculates the offset from `rhs` to `self`.
    ///
    /// Performs saturating subtraction. If `rhs` is greater than `self`,
    /// the result will be 0.
    fn sub(self, rhs: PhysAddr) -> Self::Output {
        self.0.saturating_sub(rhs.0)
    }
}


// Implement standard formatting traits for addresses.
impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use a more descriptive debug output.
        f.debug_tuple("PhysAddr")
            .field(&format_args!("0x{:x}", self.0))
            .finish()
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:x}", self.0)
    }
}

impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// --- KUnit Test Suite ---
#[kunit_tests(kernel_physadrmod)]
mod tests {
    use super::*;
    use crate::bindings::phys_addr_t;

    #[test]
    fn test_creation_and_conversion() -> Result {
        let addr = PhysAddr::from_raw(0x1000);
        assert_eq!(addr.as_raw(), 0x1000);

        let default_addr = PhysAddr::default();
        assert_eq!(default_addr.as_raw(), 0);
        assert_eq!(default_addr, PhysAddr::ZERO);

        Ok(())
    }

    #[test]
    fn test_is_null() -> Result {
        assert!(PhysAddr::ZERO.is_null());
        assert!(PhysAddr::from_raw(0).is_null());
        assert!(!PhysAddr::from_raw(1).is_null());
        Ok(())
    }

    #[test]
    fn test_arithmetic() -> Result {
        let addr = PhysAddr::from_raw(0x1000);

        // Checked addition
        assert_eq!(addr.checked_add(0x10), Some(PhysAddr::from_raw(0x1010)));
        let max_addr = PhysAddr::from_raw(phys_addr_t::MAX);
        assert_eq!(max_addr.checked_add(1), None);

        // Wrapping addition
        assert_eq!(addr.wrapping_add(0x10), PhysAddr::from_raw(0x1010));
        assert_eq!(max_addr.wrapping_add(1), PhysAddr::from_raw(0));
        assert_eq!(max_addr + 2, PhysAddr::from_raw(1));

        // Wrapping subtraction
        assert_eq!(addr.wrapping_sub(0x10), PhysAddr::from_raw(0x0ff0));
        let zero_addr = PhysAddr::from_raw(0);
        assert_eq!(zero_addr.wrapping_sub(1), PhysAddr::from_raw(phys_addr_t::MAX));
        assert_eq!(zero_addr - 2, PhysAddr::from_raw(phys_addr_t::MAX - 1));

        Ok(())
    }

    #[test]
    fn test_address_difference() -> Result {
        let addr1 = PhysAddr::from_raw(0x1000);
        let addr2 = PhysAddr::from_raw(0x2000);

        assert_eq!(addr2 - addr1, 0x1000);
        assert_eq!(addr1 - addr2, 0); // Saturating subtraction
        assert_eq!(addr1 - addr1, 0);

        Ok(())
    }

    #[test]
    fn test_alignment() -> Result {
        let addr = PhysAddr::from_raw(0x1007);
        let align = 8 as phys_addr_t;

        // align_down
        assert_eq!(addr.align_down(align), PhysAddr::from_raw(0x1000));
        let aligned_addr = PhysAddr::from_raw(0x1008);
        assert_eq!(aligned_addr.align_down(align), aligned_addr);

        // align_up
        assert_eq!(addr.align_up(align), PhysAddr::from_raw(0x1008));
        assert_eq!(aligned_addr.align_up(align), aligned_addr);
        assert_eq!(
            PhysAddr::from_raw(0).align_up(align),
            PhysAddr::from_raw(0)
        );

        Ok(())
    }
}
