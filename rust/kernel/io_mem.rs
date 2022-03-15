// SPDX-License-Identifier: GPL-2.0

//! Memory-mapped IO.
//!
//! C header: [`include/asm-generic/io.h`](../../../../include/asm-generic/io.h)

#![allow(dead_code)]

use crate::{bindings, iopoll, Error, Result};
use core::convert::TryInto;

/// Represents a memory resource.
pub struct Resource {
    offset: bindings::resource_size_t,
    size: bindings::resource_size_t,
}

impl Resource {
    pub(crate) fn new(
        start: bindings::resource_size_t,
        end: bindings::resource_size_t,
    ) -> Option<Self> {
        if start == 0 {
            return None;
        }
        Some(Self {
            offset: start,
            size: end.checked_sub(start)?.checked_add(1)?,
        })
    }
}

/// Represents a memory block of at least `SIZE` bytes.
///
/// # Invariants
///
/// `ptr` is a non-null and valid address of at least `SIZE` bytes and returned by an `ioremap`
/// variant. `ptr` is also 8-byte aligned.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::io_mem::{IoMem, Resource};
///
/// fn test(res: Resource) -> Result {
///     // Create an io mem block of at least 100 bytes.
///     // SAFETY: No DMA operations are initiated through `mem`.
///     let mem = unsafe { IoMem::<100>::try_new(res) }?;
///
///     // Read one byte from offset 10.
///     let v = mem.readb(10);
///
///     // Write value to offset 20.
///     mem.writeb(v, 20);
///
///     Ok(())
/// }
///
/// ```
pub struct IoMem<const SIZE: usize> {
    ptr: usize,
}

macro_rules! define_read {
    ($(#[$attr:meta])* $name:ident, $try_name:ident, $type_name:ty) => {
        /// Reads IO data from the given offset known, at compile time.
        ///
        /// If the offset is not known at compile time, the build will fail.
        $(#[$attr])*
        pub fn $name(&self, offset: usize) -> $type_name {
            Self::check_offset::<$type_name>(offset);
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // guarantees that the code won't build if `offset` makes the read go out of bounds
            // (including the type size).
            unsafe { bindings::$name(ptr as _) }
        }

        /// Reads IO data from the given offset.
        ///
        /// It fails if/when the offset (plus the type size) is out of bounds.
        $(#[$attr])*
        pub fn $try_name(&self, offset: usize) -> Result<$type_name> {
            if !Self::offset_ok::<$type_name>(offset) {
                return Err(Error::EINVAL);
            }
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // returns an error if `offset` would make the read go out of bounds (including the
            // type size).
            Ok(unsafe { bindings::$name(ptr as _) })
        }
    };
}

macro_rules! define_write {
    ($(#[$attr:meta])* $name:ident, $try_name:ident, $type_name:ty) => {
        /// Writes IO data to the given offset, known at compile time.
        ///
        /// If the offset is not known at compile time, the build will fail.
        $(#[$attr])*
        pub fn $name(&self, value: $type_name, offset: usize) {
            Self::check_offset::<$type_name>(offset);
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // guarantees that the code won't link if `offset` makes the write go out of bounds
            // (including the type size).
            unsafe { bindings::$name(value, ptr as _) }
        }

        /// Writes IO data to the given offset.
        ///
        /// It fails if/when the offset (plus the type size) is out of bounds.
        $(#[$attr])*
        pub fn $try_name(&self, value: $type_name, offset: usize) -> Result {
            if !Self::offset_ok::<$type_name>(offset) {
                return Err(Error::EINVAL);
            }
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // returns an error if `offset` would make the write go out of bounds (including the
            // type size).
            unsafe { bindings::$name(value, ptr as _) };
            Ok(())
        }
    };
}

macro_rules! define_readx_poll_timeout {
    ($(#[$attr:meta])* $fname:ident, $try_name:ident, $callback_name:ident, $type_name:ty) => {
        /// Polls i/o data from the given offset known, at compile time.
        ///
        /// The i/o data is polled until it matches specific condition or fails
        /// at timeout.
        ///
        /// If the offset is not known at compile time, the build will fail.
        $(#[$attr])*
        pub fn $fname<F: Fn(&$type_name) -> bool>(
            &self,
            offset: usize,
            cond: F,
            sleep_us: usize,
            timeout_us: u64,
        ) -> Result<$type_name> {
            Self::check_offset::<$type_name>(offset);
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // guarantees that the code won't build if `offset` makes the read go out of bounds
            // (including the type size).
            unsafe { iopoll::readx_poll_timeout(bindings::$callback_name, cond, sleep_us, timeout_us, ptr as *const _) }
        }

        /// Polls i/o data from the given offset known, at compile time.
        ///
        /// The i/o data is polled until it matches specific condition or fails
        /// at timeout.
        ///
        /// It fails if/when the offset (plus the type size) is out of bounds.
        $(#[$attr])*
        pub fn $try_name<F: Fn(&$type_name) -> bool>(
            &self,
            offset: usize,
            cond: F,
            sleep_us: usize,
            timeout_us: u64,
        ) -> Result<$type_name> {
            if !Self::offset_ok::<$type_name>(offset) {
                return Err(Error::EINVAL);
            }
            let ptr = self.ptr.wrapping_add(offset);
            // SAFETY: The type invariants guarantee that `ptr` is a valid pointer. The check above
            // guarantees that the code won't build if `offset` makes the read go out of bounds
            // (including the type size).
            unsafe { iopoll::readx_poll_timeout(bindings::$callback_name, cond, sleep_us, timeout_us, ptr as *const _) }
        }
    };
}

impl<const SIZE: usize> IoMem<SIZE> {
    /// Tries to create a new instance of a memory block.
    ///
    /// The resource described by `res` is mapped into the CPU's address space so that it can be
    /// accessed directly. It is also consumed by this function so that it can't be mapped again
    /// to a different address.
    ///
    /// # Safety
    ///
    /// Callers must ensure that either (a) the resulting interface cannot be used to initiate DMA
    /// operations, or (b) that DMA operations initiated via the returned interface use DMA handles
    /// allocated through the `dma` module.
    pub unsafe fn try_new(res: Resource) -> Result<Self> {
        // Check that the resource has at least `SIZE` bytes in it.
        if res.size < SIZE.try_into()? {
            return Err(Error::EINVAL);
        }

        // To be able to check pointers at compile time based only on offsets, we need to guarantee
        // that the base pointer is minimally aligned. So we conservatively expect at least 8 bytes.
        if res.offset % 8 != 0 {
            crate::pr_err!("Physical address is not 64-bit aligned: {:x}", res.offset);
            return Err(Error::EDOM);
        }

        // Try to map the resource.
        // SAFETY: Just mapping the memory range.
        let addr = unsafe { bindings::ioremap(res.offset, res.size as _) };
        if addr.is_null() {
            Err(Error::ENOMEM)
        } else {
            // INVARIANT: `addr` is non-null and was returned by `ioremap`, so it is valid. It is
            // also 8-byte aligned because we checked it above.
            Ok(Self { ptr: addr as usize })
        }
    }

    const fn offset_ok<T>(offset: usize) -> bool {
        let type_size = core::mem::size_of::<T>();
        if let Some(end) = offset.checked_add(type_size) {
            end <= SIZE && offset % type_size == 0
        } else {
            false
        }
    }

    fn offset_ok_of_val<T: ?Sized>(offset: usize, value: &T) -> bool {
        let value_size = core::mem::size_of_val(value);
        let value_alignment = core::mem::align_of_val(value);
        if let Some(end) = offset.checked_add(value_size) {
            end <= SIZE && offset % value_alignment == 0
        } else {
            false
        }
    }

    const fn check_offset<T>(offset: usize) {
        crate::build_assert!(Self::offset_ok::<T>(offset), "IoMem offset overflow");
    }

    define_readx_poll_timeout!(readb_poll_timeout, try_readb_poll_timeout, readb, u8);
    define_readx_poll_timeout!(readw_poll_timeout, try_readw_poll_timeout, readw, u16);
    define_readx_poll_timeout!(readl_poll_timeout, try_readl_poll_timeout, readl, u32);
    define_readx_poll_timeout!(
        #[cfg(CONFIG_64BIT)]
        readq_poll_timeout,
        try_readq_poll_timeout,
        readq,
        u64
    );

    define_read!(readb, try_readb, u8);
    define_read!(readw, try_readw, u16);
    define_read!(readl, try_readl, u32);
    define_read!(
        #[cfg(CONFIG_64BIT)]
        readq,
        try_readq,
        u64
    );

    define_read!(readb_relaxed, try_readb_relaxed, u8);
    define_read!(readw_relaxed, try_readw_relaxed, u16);
    define_read!(readl_relaxed, try_readl_relaxed, u32);
    define_read!(
        #[cfg(CONFIG_64BIT)]
        readq_relaxed,
        try_readq_relaxed,
        u64
    );

    define_write!(writeb, try_writeb, u8);
    define_write!(writew, try_writew, u16);
    define_write!(writel, try_writel, u32);
    define_write!(
        #[cfg(CONFIG_64BIT)]
        writeq,
        try_writeq,
        u64
    );

    define_write!(writeb_relaxed, try_writeb_relaxed, u8);
    define_write!(writew_relaxed, try_writew_relaxed, u16);
    define_write!(writel_relaxed, try_writel_relaxed, u32);
    define_write!(
        #[cfg(CONFIG_64BIT)]
        writeq_relaxed,
        try_writeq_relaxed,
        u64
    );
}

impl<const SIZE: usize> Drop for IoMem<SIZE> {
    fn drop(&mut self) {
        // SAFETY: By the type invariant, `self.ptr` is a value returned by a previous successful
        // call to `ioremap`.
        unsafe { bindings::iounmap(self.ptr as _) };
    }
}
