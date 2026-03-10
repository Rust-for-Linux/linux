// SPDX-License-Identifier: GPL-2.0

//! Types for working with the block layer.

use crate::bindings;
use crate::fs::FileSystem;
use crate::inode::INode;
use crate::types::Opaque;

pub mod mq;

/// Bit mask for masking out [`SECTOR_SIZE`].
pub const SECTOR_MASK: u32 = bindings::SECTOR_MASK;

/// Sectors are size `1 << SECTOR_SHIFT`.
pub const SECTOR_SHIFT: u32 = bindings::SECTOR_SHIFT;

/// Size of a sector.
pub const SECTOR_SIZE: u32 = bindings::SECTOR_SIZE;

/// The difference between the size of a page and the size of a sector,
/// expressed as a power of two.
pub const PAGE_SECTORS_SHIFT: u32 = bindings::PAGE_SECTORS_SHIFT;

/// The type used for indexing onto a disc or disc partition.
///
/// This is C's `sector_t`.
pub type Sector = u64;

/// The type of the inode's block count.
///
/// This is C's `blkcnt_t`.
pub type Count = u64;

/// A block device.
///
/// Wraps the kernel's `struct block_device`.
#[repr(transparent)]
pub struct Device(pub(crate) Opaque<bindings::block_device>);

impl Device {
    /// Creates a new block device reference from the given raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `ptr` is valid and remains so for the lifetime of the returned
    /// object.
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::block_device) -> &'a Self {
        // SAFETY: The safety requirements guarantee that the cast below is ok.
        unsafe { &*ptr.cast::<Self>() }
    }

    /// Returns the inode associated with this block device.
    // TODO: Maybe this should be dealt with in the Address space struct instead of here
    // Also should be default type instead of generic
    pub fn inode<T: FileSystem + ?Sized>(&self) -> &INode<T> {
        // SAFETY: `bd_mapping` is never reassigned.
        let mapping = unsafe { (*self.0.get()).bd_mapping };
        // SAFETY: `mapping` is set if device is initilized.
        let inode_ptr = unsafe { (*mapping).host };
        // SAFETY: `ptr` is valid as long as the block device remains valid as well.
        unsafe { INode::from_raw(inode_ptr) }
    }
}
