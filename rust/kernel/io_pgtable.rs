// SPDX-License-Identifier: GPL-2.0

//! IOMMU page table management
//!
//! C header: [`include/io-pgtable.h`](../../../../include/io-pgtable.h)

use crate::{
    bindings, device,
    error::{code::*, to_result, Result},
    types::{ForeignOwnable, ScopeGuard},
};

use core::marker::PhantomData;
use core::mem;
use core::num::NonZeroU64;

/// Protection flags used with IOMMU mappings.
pub mod prot {
    /// Read access.
    pub const READ: u32 = bindings::IOMMU_READ;
    /// Write access.
    pub const WRITE: u32 = bindings::IOMMU_WRITE;
    /// Request cache coherency.
    pub const CACHE: u32 = bindings::IOMMU_CACHE;
    /// Request no-execute permission.
    pub const NOEXEC: u32 = bindings::IOMMU_NOEXEC;
    /// MMIO peripheral mapping.
    pub const MMIO: u32 = bindings::IOMMU_MMIO;
    /// Privileged mapping.
    pub const PRIV: u32 = bindings::IOMMU_PRIV;
}

/// Represents a requested io_pgtable configuration.
pub struct Config {
    /// Quirk bitmask (type-specific).
    pub quirks: usize,
    /// Valid page sizes, as a bitmask of powers of two.
    pub pgsize_bitmap: usize,
    /// Input address space size in bits.
    pub ias: usize,
    /// Output address space size in bits.
    pub oas: usize,
    /// IOMMU uses coherent accesses for page table walks.
    pub coherent_walk: bool,
}

/// IOMMU callbacks for TLB and page table management.
///
/// Users must implement this trait to perform the TLB flush actions for this IOMMU, if
/// required.
pub trait FlushOps {
    /// User-specified type owned by the IOPagetable that will be passed to TLB operations.
    type Data: ForeignOwnable + Send + Sync;

    /// Synchronously invalidate the entire TLB context.
    fn tlb_flush_all(data: <Self::Data as ForeignOwnable>::Borrowed<'_>);

    /// Synchronously invalidate all intermediate TLB state (sometimes referred to as the "walk
    /// cache") for a virtual address range.
    fn tlb_flush_walk(
        data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        iova: usize,
        size: usize,
        granule: usize,
    );

    /// Optional callback to queue up leaf TLB invalidation for a single page.
    ///
    /// IOMMUs that cannot batch TLB invalidation operations efficiently will typically issue
    /// them here, but others may decide to update the iommu_iotlb_gather structure and defer
    /// the invalidation until iommu_iotlb_sync() instead.
    ///
    /// TODO: Implement the gather argument for batching.
    fn tlb_add_page(
        data: <Self::Data as ForeignOwnable>::Borrowed<'_>,
        iova: usize,
        granule: usize,
    );
}

/// Inner page table info shared across all table types.
/// # Invariants
///
///   - [`self.ops`] is valid and non-null.
///   - [`self.cfg`] is valid and non-null.
#[doc(hidden)]
pub struct IoPageTableInner {
    ops: *mut bindings::io_pgtable_ops,
    cfg: bindings::io_pgtable_cfg,
    data: *mut core::ffi::c_void,
}

/// Helper trait to get the config type for a single page table type from the union.
pub trait GetConfig {
    /// Returns the specific output configuration for this page table type.
    fn cfg(iopt: &impl IoPageTable) -> &Self
    where
        Self: Sized;
}

/// A generic IOMMU page table
pub trait IoPageTable: crate::private::Sealed {
    #[doc(hidden)]
    const FLUSH_OPS: bindings::iommu_flush_ops;

    #[doc(hidden)]
    fn new_fmt<T: FlushOps>(
        dev: &dyn device::RawDevice,
        format: u32,
        config: Config,
        data: T::Data,
    ) -> Result<IoPageTableInner> {
        let ptr = data.into_foreign() as *mut _;
        let guard = ScopeGuard::new(|| {
            // SAFETY: `ptr` came from a previous call to `into_foreign`.
            unsafe { T::Data::from_foreign(ptr) };
        });

        let mut raw_cfg = bindings::io_pgtable_cfg {
            quirks: config.quirks.try_into()?,
            pgsize_bitmap: config.pgsize_bitmap.try_into()?,
            ias: config.ias.try_into()?,
            oas: config.oas.try_into()?,
            coherent_walk: config.coherent_walk,
            tlb: &Self::FLUSH_OPS,
            iommu_dev: dev.raw_device(),
            __bindgen_anon_1: unsafe { mem::zeroed() },
        };

        let ops = unsafe {
            bindings::alloc_io_pgtable_ops(format as bindings::io_pgtable_fmt, &mut raw_cfg, ptr)
        };

        if ops.is_null() {
            return Err(EINVAL);
        }

        guard.dismiss();
        Ok(IoPageTableInner {
            ops,
            cfg: raw_cfg,
            data: ptr,
        })
    }

    /// Map a range of pages.
    fn map_pages(
        &mut self,
        iova: usize,
        paddr: usize,
        pgsize: usize,
        pgcount: usize,
        prot: u32,
    ) -> Result<usize> {
        let mut mapped: usize = 0;

        to_result(unsafe {
            (*self.inner_mut().ops).map_pages.unwrap()(
                self.inner_mut().ops,
                iova as u64,
                paddr as u64,
                pgsize,
                pgcount,
                prot as i32,
                bindings::GFP_KERNEL,
                &mut mapped,
            )
        })?;

        Ok(mapped)
    }

    /// Unmap a range of pages.
    fn unmap_pages(
        &mut self,
        iova: usize,
        pgsize: usize,
        pgcount: usize,
        // TODO: gather: *mut iommu_iotlb_gather,
    ) -> usize {
        unsafe {
            (*self.inner_mut().ops).unmap_pages.unwrap()(
                self.inner_mut().ops,
                iova as u64,
                pgsize,
                pgcount,
                core::ptr::null_mut(),
            )
        }
    }

    /// Translate an IOVA to the corresponding physical address, if mapped.
    fn iova_to_phys(&mut self, iova: usize) -> Option<NonZeroU64> {
        NonZeroU64::new(unsafe {
            (*self.inner_mut().ops).iova_to_phys.unwrap()(self.inner_mut().ops, iova as u64)
        })
    }

    #[doc(hidden)]
    fn inner_mut(&mut self) -> &mut IoPageTableInner;

    #[doc(hidden)]
    fn inner(&self) -> &IoPageTableInner;

    #[doc(hidden)]
    fn raw_cfg(&self) -> &bindings::io_pgtable_cfg {
        &self.inner().cfg
    }
}

unsafe impl Send for IoPageTableInner {}
unsafe impl Sync for IoPageTableInner {}

unsafe extern "C" fn tlb_flush_all_callback<T: FlushOps>(cookie: *mut core::ffi::c_void) {
    T::tlb_flush_all(unsafe { T::Data::borrow(cookie) });
}

unsafe extern "C" fn tlb_flush_walk_callback<T: FlushOps>(
    iova: core::ffi::c_ulong,
    size: usize,
    granule: usize,
    cookie: *mut core::ffi::c_void,
) {
    T::tlb_flush_walk(
        unsafe { T::Data::borrow(cookie) },
        iova as usize,
        size,
        granule,
    );
}

unsafe extern "C" fn tlb_add_page_callback<T: FlushOps>(
    _gather: *mut bindings::iommu_iotlb_gather,
    iova: core::ffi::c_ulong,
    granule: usize,
    cookie: *mut core::ffi::c_void,
) {
    T::tlb_add_page(unsafe { T::Data::borrow(cookie) }, iova as usize, granule);
}

macro_rules! iopt_cfg {
    ($name:ident, $field:ident, $type:ident) => {
        /// An IOMMU page table configuration for a specific kind of pagetable.
        pub type $name = bindings::$type;

        impl GetConfig for $name {
            fn cfg(iopt: &impl IoPageTable) -> &$name {
                unsafe { &iopt.raw_cfg().__bindgen_anon_1.$field }
            }
        }
    };
}

impl GetConfig for () {
    fn cfg(_iopt: &impl IoPageTable) -> &() {
        &()
    }
}

macro_rules! iopt_type {
    ($type:ident, $cfg:ty, $fmt:ident) => {
        /// Represents an IOPagetable of this type.
        pub struct $type<T: FlushOps>(IoPageTableInner, PhantomData<T>);

        impl<T: FlushOps> $type<T> {
            /// Creates a new IOPagetable implementation of this type.
            pub fn new(dev: &dyn device::RawDevice, config: Config, data: T::Data) -> Result<Self> {
                Ok(Self(
                    <Self as IoPageTable>::new_fmt::<T>(dev, bindings::$fmt, config, data)?,
                    PhantomData,
                ))
            }

            /// Get the configuration for this IOPagetable.
            pub fn cfg(&self) -> &$cfg {
                <$cfg as GetConfig>::cfg(self)
            }
        }

        impl<T: FlushOps> crate::private::Sealed for $type<T> {}

        impl<T: FlushOps> IoPageTable for $type<T> {
            const FLUSH_OPS: bindings::iommu_flush_ops = bindings::iommu_flush_ops {
                tlb_flush_all: Some(tlb_flush_all_callback::<T>),
                tlb_flush_walk: Some(tlb_flush_walk_callback::<T>),
                tlb_add_page: Some(tlb_add_page_callback::<T>),
            };

            fn inner(&self) -> &IoPageTableInner {
                &self.0
            }

            fn inner_mut(&mut self) -> &mut IoPageTableInner {
                &mut self.0
            }
        }

        impl<T: FlushOps> Drop for $type<T> {
            fn drop(&mut self) {
                // SAFETY: The pointer is valid by the type invariant.
                unsafe { bindings::free_io_pgtable_ops(self.0.ops) };

                // Free context data.
                //
                // SAFETY: This matches the call to `into_foreign` from `new` in the success case.
                unsafe { T::Data::from_foreign(self.0.data) };
            }
        }
    };
}

// Ew...
iopt_cfg!(
    ARMLPAES1Cfg,
    arm_lpae_s1_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_1
);
iopt_cfg!(
    ARMLPAES2Cfg,
    arm_lpae_s2_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_2
);
iopt_cfg!(
    ARMv7SCfg,
    arm_v7s_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_3
);
iopt_cfg!(
    ARMMaliLPAECfg,
    arm_mali_lpae_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_4
);
iopt_cfg!(
    AppleDARTCfg,
    apple_dart_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_5
);
iopt_cfg!(
    AppleUATCfg,
    apple_uat_cfg,
    io_pgtable_cfg__bindgen_ty_1__bindgen_ty_6
);

iopt_type!(ARM32LPAES1, ARMLPAES1Cfg, io_pgtable_fmt_ARM_32_LPAE_S1);
iopt_type!(ARM32LPAES2, ARMLPAES2Cfg, io_pgtable_fmt_ARM_32_LPAE_S2);
iopt_type!(ARM64LPAES1, ARMLPAES1Cfg, io_pgtable_fmt_ARM_64_LPAE_S1);
iopt_type!(ARM64LPAES2, ARMLPAES2Cfg, io_pgtable_fmt_ARM_64_LPAE_S2);
iopt_type!(ARMv7S, ARMv7SCfg, io_pgtable_fmt_ARM_V7S);
iopt_type!(ARMMaliLPAE, ARMMaliLPAECfg, io_pgtable_fmt_ARM_MALI_LPAE);
iopt_type!(AMDIOMMUV1, (), io_pgtable_fmt_AMD_IOMMU_V1);
iopt_type!(AppleDART, AppleDARTCfg, io_pgtable_fmt_APPLE_DART);
iopt_type!(AppleDART2, AppleDARTCfg, io_pgtable_fmt_APPLE_DART2);
iopt_type!(AppleUAT, AppleUATCfg, io_pgtable_fmt_APPLE_UAT);
