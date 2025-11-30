//! File system io maps.
//!
//! This module allows Rust code to use iomaps to implement filesystems.
//!
//! C headers: [`include/linux/iomap.h`](srctree/include/linux/iomap.h)

use super::address_space;
use crate::pr_info;
use crate::prelude::EIO;
use crate::{
    error::{from_result, Result},
    folio::Folio,
    folio::PageCache,
    fs::file::File,
    fs::Offset,
    fs::FileSystem,
    types::Locked,
};

use crate::inode::INode;
use core::marker::PhantomData;
use core::mem;
use macros::vtable;
use uapi::writeback_control;


/// A map from address space to block device.
#[repr(transparent)]
pub struct Map<'a>(pub bindings::iomap, PhantomData<&'a ()>);

impl<'a> Map<'a> {
    // /// Sets the map type.
    // pub fn set_type(&mut self, t: Type) -> &mut Self {
    //     self.0.type_ = t as u16;
    //     self
    // }
    //
    // /// Sets the file offset, in bytes.
    // pub fn set_offset(&mut self, v: Offset) -> &mut Self {
    //     self.0.offset = v;
    //     self
    // }
    //
    // /// Sets the length of the mapping, in bytes.
    // pub fn set_length(&mut self, len: u64) -> &mut Self {
    //     self.0.length = len;
    //     self
    // }
    //
    // /// Sets the mapping flags.
    // ///
    // /// Values come from the [`map_flags`] module.
    // pub fn set_flags(&mut self, flags: u16) -> &mut Self {
    //     self.0.flags = flags;
    //     self
    // }
    //
    // /// Sets the disk offset of the mapping, in bytes.
    // pub fn set_addr(&mut self, addr: u64) -> &mut Self {
    //     self.0.addr = addr;
    //     self
    // }
    //
    // /// Sets the block device of the mapping.
    // pub fn set_bdev(&mut self, bdev: Option<&'a block::Device>) -> &mut Self {
    //     self.0.bdev = if let Some(b) = bdev {
    //         b.0.get()
    //     } else {
    //         core::ptr::null_mut()
    //     };
    //     self
    // }
}

/// Operations implemented by iomap users.
pub trait Operations {
    /// File system that these operations are compatible with.
    type FileSystem: FileSystem + ?Sized;

    /// Returns the existing mapping at `pos`, or reserves space starting at `pos` for up to
    /// `length`, as long as it can be done as a single mapping. The actual length is returned in
    /// `iomap`.
    ///
    /// The values of `flags` come from the [`flags`] module.
    fn begin<'a>(
        inode: &'a INode<Self::FileSystem>,
        pos: Offset,
        length: Offset,
        flags: u32,
        map: &mut Map<'a>,
        srcmap: &mut Map<'a>,
    ) -> Result;

    /// Commits and/or unreserves space previously allocated using [`Operations::begin`]. `writte`n
    /// indicates the length of the successful write operation which needs to be commited, while
    /// the rest needs to be unreserved. `written` might be zero if no data was written.
    ///
    /// The values of `flags` come from the [`flags`] module.
    fn end<'a>(
        _inode: &'a INode<Self::FileSystem>,
        _pos: Offset,
        _length: Offset,
        _written: isize,
        _flags: u32,
        _map: &Map<'a>,
    ) -> Result {
        Ok(())
    }
}


/// Returns address space oprerations backed by iomaps.
pub const fn aops<T: Operations + ?Sized>() -> address_space::Ops<T::FileSystem> {
    struct Table<T: Operations + ?Sized>(PhantomData<T>);
    impl<T: Operations + ?Sized> Table<T> {
        const MAP_TABLE: bindings::iomap_ops = bindings::iomap_ops {
            iomap_begin: Some(Self::iomap_begin_callback),
            iomap_end: Some(Self::iomap_end_callback),
        };

        const WRITEBACK_TABLE: bindings::iomap_writeback_ops = bindings::iomap_writeback_ops {
            writeback_range: None,
            writeback_submit: None,
        };

        extern "C" fn iomap_begin_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            flags: u32,
            map: *mut bindings::iomap,
            srcmap: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                T::begin(
                    inode,
                    pos,
                    length,
                    flags,
                    // SAFETY: The C API guarantees that `map` is valid for write.
                    unsafe { &mut *map.cast::<Map<'_>>() },
                    // SAFETY: The C API guarantees that `srcmap` is valid for write.
                    unsafe { &mut *srcmap.cast::<Map<'_>>() },
                )?;
                Ok(0)
            })
        }

        extern "C" fn iomap_end_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            written: isize,
            flags: u32,
            map: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                // SAFETY: The C API guarantees that `map` is valid for read.
                T::end(inode, pos, length, written, flags, unsafe {
                    &*map.cast::<Map<'_>>()
                })?;
                Ok(0)
            })
        }

        const TABLE: bindings::address_space_operations = bindings::address_space_operations {
            read_folio: Some(Self::read_folio_callback),
            writepages: Some(Self::writepages_callback),
            dirty_folio: Some(bindings::iomap_dirty_folio),
            // readahead: Some(Self::readahead_callback),
            readahead: None,
            write_begin: None,
            write_end: None,
            // bmap: Some(Self::bmap_callback),
            bmap: None,
            invalidate_folio: Some(bindings::iomap_invalidate_folio),
            release_folio: Some(bindings::iomap_release_folio),
            free_folio: None,
            // direct_IO: Some(bindings::noop_direct_IO),
            direct_IO: None,
            migrate_folio: None,
            launder_folio: None,
            is_partially_uptodate: None,
            is_dirty_writeback: None,
            error_remove_folio: None,
            swap_activate: None,
            swap_deactivate: None,
            swap_rw: None,
        };

        extern "C" fn read_folio_callback(
            _file: *mut bindings::file,
            folio: *mut bindings::folio,
        ) -> i32 {
            // SAFETY: `folio` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_read_folio(folio, &Self::MAP_TABLE) }
        }


        extern "C" fn writepages_callback(_mapping: *mut bindings::address_space, _wbc: *mut bindings::writeback_control) -> i32 {
            // Safety: iomap docs say wpc must be zero-initialized.
            let mut wpc: bindings::iomap_writepage_ctx = unsafe {mem::zeroed()};
            wpc.inode = unsafe { (*_mapping).host };   // struct inode *host
            wpc.wbc   = _wbc;
            wpc.ops   = &Self::WRITEBACK_TABLE;

            unsafe {bindings::iomap_writepages(&mut wpc)}
        }

        extern "C" fn readahead_callback(rac: *mut bindings::readahead_control) {
            // SAFETY: `rac` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_readahead(rac, &Self::MAP_TABLE) }
        }

        extern "C" fn bmap_callback(mapping: *mut bindings::address_space, block: u64) -> u64 {
            // SAFETY: `mapping` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_bmap(mapping, block, &Self::MAP_TABLE) }
        }
    }
    address_space::Ops(&Table::<T>::TABLE, PhantomData)
}
