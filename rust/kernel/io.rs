// SPDX-License-Identifier: GPL-2.0

//! Memory-mapped IO.
//!
//! C header: [`include/asm-generic/io.h`](srctree/include/asm-generic/io.h)

use crate::error::{code::EINVAL, Result};
use crate::{bindings, build_assert};

/// IO-mapped memory, starting at the base address @addr and spanning @maxlen bytes.
///
/// The creator (usually a subsystem / bus such as PCI) is responsible for creating the
/// mapping, performing an additional region request etc.
///
/// # Invariant
///
/// `addr` is the start and `maxsize` the length of valid I/O mapped memory region of size
/// `maxsize`.
///
/// # Examples
///
/// ```no_run
/// # use kernel::{bindings, io::Io};
/// # use core::ops::Deref;
///
/// // See also [`pci::Bar`] for a real example.
/// struct IoMem<const SIZE: usize>(Io<SIZE>);
///
/// impl<const SIZE: usize> IoMem<SIZE> {
///     /// # Safety
///     ///
///     /// [`paddr`, `paddr` + `SIZE`) must be a valid MMIO region that is mappable into the CPUs
///     /// virtual address space.
///     unsafe fn new(paddr: usize) -> Result<Self>{
///
///         // SAFETY: By the safety requirements of this function [`paddr`, `paddr` + `SIZE`) is
///         // valid for `ioremap`.
///         let addr = unsafe { bindings::ioremap(paddr as _, SIZE.try_into().unwrap()) };
///         if addr.is_null() {
///             return Err(ENOMEM);
///         }
///
///         // SAFETY: `addr` is guaranteed to be the start of a valid I/O mapped memory region of
///         // size `SIZE`.
///         let io = unsafe { Io::new(addr as _, SIZE)? };
///
///         Ok(IoMem(io))
///     }
/// }
///
/// impl<const SIZE: usize> Drop for IoMem<SIZE> {
///     fn drop(&mut self) {
///         // SAFETY: Safe as by the invariant of `Io`.
///         unsafe { bindings::iounmap(self.0.base_addr() as _); };
///     }
/// }
///
/// impl<const SIZE: usize> Deref for IoMem<SIZE> {
///    type Target = Io<SIZE>;
///
///    fn deref(&self) -> &Self::Target {
///        &self.0
///    }
/// }
///
///# fn no_run() -> Result<(), Error> {
/// // SAFETY: Invalid usage for example purposes.
/// let iomem = unsafe { IoMem::<{ core::mem::size_of::<u32>() }>::new(0xBAAAAAAD)? };
/// iomem.writel(0x42, 0x0);
/// assert!(iomem.try_writel(0x42, 0x0).is_ok());
/// assert!(iomem.try_writel(0x42, 0x4).is_err());
/// # Ok(())
/// # }
/// ```
pub struct Io<const SIZE: usize = 0> {
    addr: usize,
    maxsize: usize,
}

macro_rules! define_read {
    ($(#[$attr:meta])* $name:ident, $try_name:ident, $type_name:ty) => {
        /// Read IO data from a given offset known at compile time.
        ///
        /// Bound checks are performed on compile time, hence if the offset is not known at compile
        /// time, the build will fail.
        $(#[$attr])*
        #[inline]
        pub fn $name(&self, offset: usize) -> $type_name {
            let addr = self.io_addr_assert::<$type_name>(offset);

            // SAFETY: By the type invariant `addr` is a valid address for MMIO operations.
            unsafe { bindings::$name(addr as _) }
        }

        /// Read IO data from a given offset.
        ///
        /// Bound checks are performed on runtime, it fails if the offset (plus the type size) is
        /// out of bounds.
        $(#[$attr])*
        pub fn $try_name(&self, offset: usize) -> Result<$type_name> {
            let addr = self.io_addr::<$type_name>(offset)?;

            // SAFETY: By the type invariant `addr` is a valid address for MMIO operations.
            Ok(unsafe { bindings::$name(addr as _) })
        }
    };
}

macro_rules! define_write {
    ($(#[$attr:meta])* $name:ident, $try_name:ident, $type_name:ty) => {
        /// Write IO data from a given offset known at compile time.
        ///
        /// Bound checks are performed on compile time, hence if the offset is not known at compile
        /// time, the build will fail.
        $(#[$attr])*
        #[inline]
        pub fn $name(&self, value: $type_name, offset: usize) {
            let addr = self.io_addr_assert::<$type_name>(offset);

            // SAFETY: By the type invariant `addr` is a valid address for MMIO operations.
            unsafe { bindings::$name(value, addr as _, ) }
        }

        /// Write IO data from a given offset.
        ///
        /// Bound checks are performed on runtime, it fails if the offset (plus the type size) is
        /// out of bounds.
        $(#[$attr])*
        pub fn $try_name(&self, value: $type_name, offset: usize) -> Result {
            let addr = self.io_addr::<$type_name>(offset)?;

            // SAFETY: By the type invariant `addr` is a valid address for MMIO operations.
            unsafe { bindings::$name(value, addr as _) }
            Ok(())
        }
    };
}

impl<const SIZE: usize> Io<SIZE> {
    ///
    ///
    /// # Safety
    ///
    /// Callers must ensure that `addr` is the start of a valid I/O mapped memory region of size
    /// `maxsize`.
    pub unsafe fn new(addr: usize, maxsize: usize) -> Result<Self> {
        if maxsize < SIZE {
            return Err(EINVAL);
        }

        // INVARIANT: Covered by the safety requirements of this function.
        Ok(Self { addr, maxsize })
    }

    /// Returns the base address of this mapping.
    #[inline]
    pub fn base_addr(&self) -> usize {
        self.addr
    }

    /// Returns the size of this mapping.
    #[inline]
    pub fn maxsize(&self) -> usize {
        self.maxsize
    }

    #[inline]
    const fn offset_valid<U>(offset: usize, size: usize) -> bool {
        let type_size = core::mem::size_of::<U>();
        if let Some(end) = offset.checked_add(type_size) {
            end <= size && offset % type_size == 0
        } else {
            false
        }
    }

    #[inline]
    fn io_addr<U>(&self, offset: usize) -> Result<usize> {
        if !Self::offset_valid::<U>(offset, self.maxsize()) {
            return Err(EINVAL);
        }

        // Probably no need to check, since the safety requirements of `Self::new` guarantee that
        // this can't overflow.
        self.base_addr().checked_add(offset).ok_or(EINVAL)
    }

    #[inline]
    fn io_addr_assert<U>(&self, offset: usize) -> usize {
        build_assert!(Self::offset_valid::<U>(offset, SIZE));

        self.base_addr() + offset
    }

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
