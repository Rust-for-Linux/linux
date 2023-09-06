// SPDX-License-Identifier: GPL-2.0

//! DMA mapping.
//!
//! C header: [`include/linux/dma-mapping.h`](../../../../include/linux/dma-mapping.h)

#![allow(dead_code)]

use crate::{bindings, device, device::RawDevice, error, to_result, Result};

/// Set the DMA mask to inform the kernel about DMA addressing capabilities.
pub fn set_mask(dev: &dyn device::RawDevice, mask: u64) -> Result {
    to_result(unsafe { bindings::dma_set_mask(dev.raw_device(), mask) })
}

/// Set the DMA coherent mask to inform the kernel about DMA addressing capabilities.
pub fn set_coherent_mask(dev: &dyn device::RawDevice, mask: u64) -> Result {
    to_result(unsafe { bindings::dma_set_coherent_mask(dev.raw_device(), mask) })
}

/// Information about allocated DMA-coherent memory.
pub struct Allocation<T> {
    dev: device::Device,
    size: usize,
    /// DMA address
    pub dma_handle: bindings::dma_addr_t,
    /// processor memory
    pub cpu_addr: *mut T,
}

impl<T> Allocation<T> {
    /// Alloc DMA-coherent memory.
    pub fn try_new(
        dev: &dyn device::RawDevice,
        size: usize,
        flag: bindings::gfp_t,
    ) -> Result<Allocation<T>> {
        let mut dma_handle = 0;
        // SAFETY: dev.raw_device() is guaranteed to be valid.
        let ptr =
            unsafe { bindings::dma_alloc_coherent(dev.raw_device(), size, &mut dma_handle, flag) };
        if ptr.is_null() {
            Err(error::code::ENOMEM)
        } else {
            Ok(Allocation {
                dev: device::Device::from_dev(dev),
                size,
                dma_handle,
                cpu_addr: ptr as _,
            })
        }
    }
}

impl<T> Drop for Allocation<T> {
    fn drop(&mut self) {
        // SAFETY: Allocation holds a reference to the device so self.dev.raw_device() is valid.
        unsafe {
            bindings::dma_free_coherent(
                self.dev.raw_device(),
                self.size,
                self.cpu_addr as _,
                self.dma_handle,
            )
        }
    }
}

/// Information about mapped single processor memory.
pub struct MapSingle<T> {
    dev: device::Device,
    size: usize,
    /// DMA address
    pub dma_handle: bindings::dma_addr_t,
    cpu_addr: *mut T,
    dir: bindings::dma_data_direction,
}

impl<T> MapSingle<T> {
    /// Map single processor memory.
    pub fn try_new(
        dev: &dyn device::RawDevice,
        ptr: *mut T,
        size: core::ffi::c_size_t,
        dir: bindings::dma_data_direction,
    ) -> Result<MapSingle<T>> {
        // SAFETY: dev.raw_device() is guaranteed to be valid.
        unsafe {
            let raw_dev = dev.raw_device();
            let dma_handle = bindings::dma_map_single_attrs(raw_dev, ptr as _, size, dir, 0);
            if bindings::dma_mapping_error(raw_dev, dma_handle) != 0 {
                Err(error::code::ENOMEM)
            } else {
                Ok(MapSingle {
                    dev: device::Device::from_dev(dev),
                    size,
                    dma_handle,
                    cpu_addr: ptr as _,
                    dir,
                })
            }
        }
    }
}

impl<T> Drop for MapSingle<T> {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: Allocation holds a reference to the device so self.dev.raw_device() is valid.
            bindings::dma_unmap_single_attrs(
                self.dev.raw_device(),
                self.cpu_addr as _,
                self.size,
                self.dir,
                0,
            )
        }
    }
}
