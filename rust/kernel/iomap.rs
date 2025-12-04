//! File system io maps.
//!
//! This module allows Rust code to use iomaps to implement filesystems.
//!
//! C headers: [`include/linux/iomap.h`](srctree/include/linux/iomap.h)

use super::address_space;
use crate::error::{to_result, Error};
use crate::iov::IovIterSource;
use crate::pr_info;
use crate::prelude::{EIO, EOVERFLOW};
use crate::{
    block,
    error::{from_result, Result},
    folio::{Folio, PageCache},
    fs::{file::File, FileSystem, Kiocb, Offset},
    types::{ForeignOwnable, Locked},
};

use crate::inode::INode;
use core::marker::PhantomData;
use core::mem;
use core::ptr;
use macros::vtable;
use uapi::writeback_control;

/// The type of mapping.
///
/// This is used in [`Map`].
#[repr(u16)]
pub enum Type {
    /// No blocks allocated, need allocation.
    Hole = bindings::IOMAP_HOLE as u16,

    /// Delayed allocation blocks.
    DelAlloc = bindings::IOMAP_DELALLOC as u16,

    /// Blocks allocated at the given address.
    Mapped = bindings::IOMAP_MAPPED as u16,

    /// Blocks allocated at the given address in unwritten state.
    Unwritten = bindings::IOMAP_UNWRITTEN as u16,

    /// Data inline in the inode.
    Inline = bindings::IOMAP_INLINE as u16,
}

impl From<Type> for u16 {
    fn from(t: Type) -> Self {
        // This is ok as Type values are is represented as u16
        t as u16
    }
}

/// Flags usable in [`Map`], in [`Map::set_flags`] in particular.
pub mod map_flags {
    /// Indicates that the blocks have been newly allocated and need zeroing for areas that no data
    /// is copied to.
    pub const NEW: u16 = bindings::IOMAP_F_NEW as u16;

    /// Indicates that the inode has uncommitted metadata needed to access written data and
    /// requires fdatasync to commit them to persistent storage. This needs to take into account
    /// metadata changes that *may* be made at IO completion, such as file size updates from direct
    /// IO.
    pub const DIRTY: u16 = bindings::IOMAP_F_DIRTY as u16;

    /// Indicates that the blocks are shared, and will need to be unshared as part a write.
    pub const SHARED: u16 = bindings::IOMAP_F_SHARED as u16;

    /// Indicates that the iomap contains the merge of multiple block mappings.
    pub const MERGED: u16 = bindings::IOMAP_F_MERGED as u16;

    /// Indicates that the file system requires the use of buffer heads for this mapping.
    pub const BUFFER_HEAD: u16 = bindings::IOMAP_F_BUFFER_HEAD as u16;

    /// Indicates that the iomap is for an extended attribute extent rather than a file data
    /// extent.
    pub const XATTR: u16 = bindings::IOMAP_F_XATTR as u16;

    /// Indicates to the iomap_end method that the file size has changed as the result of this
    /// write operation.
    pub const SIZE_CHANGED: u16 = bindings::IOMAP_F_SIZE_CHANGED as u16;

    /// Indicates that the iomap is not valid any longer and the file range it covers needs to be
    /// remapped by the high level before the operation can proceed.
    pub const STALE: u16 = bindings::IOMAP_F_STALE as u16;

    /// Flags from 0x1000 up are for file system specific usage.
    pub const PRIVATE: u16 = bindings::IOMAP_F_PRIVATE as u16;
}

/// A map from address space to block device.
#[repr(transparent)]
pub struct Map<'a>(pub bindings::iomap, PhantomData<&'a ()>);

impl<'a> Map<'a> {
    /// Sets the map type.
    pub fn set_type(&mut self, t: Type) -> &mut Self {
        self.0.type_ = t.into();
        self
    }

    /// Sets the file offset, in bytes.
    pub fn set_offset(&mut self, v: Offset) -> &mut Self {
        self.0.offset = v;
        self
    }

    /// Sets the length of the mapping, in bytes.
    pub fn set_length(&mut self, len: u64) -> &mut Self {
        self.0.length = len;
        self
    }

    /// Sets the mapping flags.
    ///
    /// Values come from the [`map_flags`] module.
    pub fn set_flags(&mut self, flags: u16) -> &mut Self {
        self.0.flags = flags;
        self
    }

    /// Sets the disk offset of the mapping, in bytes.
    pub fn set_addr(&mut self, addr: u64) -> &mut Self {
        self.0.addr = addr;
        self
    }

    /// Sets the block device of the mapping.
    pub fn set_bdev(&mut self, bdev: Option<&'a block::Device>) -> &mut Self {
        self.0.bdev = if let Some(b) = bdev {
            b.0.get()
        } else {
            core::ptr::null_mut()
        };
        self
    }
}
/// Flags passed to [`Operations::begin`] and [`Operations::end`].
pub mod flags {
    /// Writing, must allocate block.
    pub const WRITE: u32 = bindings::IOMAP_WRITE;

    /// Zeroing operation, may skip holes.
    pub const ZERO: u32 = bindings::IOMAP_ZERO;

    /// Report extent status, e.g. FIEMAP.
    pub const REPORT: u32 = bindings::IOMAP_REPORT;

    /// Mapping for page fault.
    pub const FAULT: u32 = bindings::IOMAP_FAULT;

    /// Direct I/O.
    pub const DIRECT: u32 = bindings::IOMAP_DIRECT;

    /// Do not block.
    pub const NOWAIT: u32 = bindings::IOMAP_NOWAIT;

    /// Only pure overwrites allowed.
    pub const OVERWRITE_ONLY: u32 = bindings::IOMAP_OVERWRITE_ONLY;

    /// `unshare_file_range`.
    pub const UNSHARE: u32 = bindings::IOMAP_UNSHARE;

    /// DAX mapping.
    pub const DAX: u32 = bindings::IOMAP_DAX;
}

// TODO: We should probably have this instead
// /// WritebackOperations  implemented by iomap users.
// pub trait WritebackOperations {
//     type FileSystem: FileSystem + ?Sized;
//
//     fn writeback_range<'a>(folio: &'a Folio<Self::FileSystem>) -> Result;
//
//     fn writeback_submit<'a>() -> Result;
// }

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
        srcmap: Option<&mut Map<'a>>,
    ) -> Result;

    /// Commits and/or unreserves space previously allocated using [`Operations::begin`]. `written`
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

struct Table<T: Operations + ?Sized>(PhantomData<T>);

impl<T: Operations + ?Sized> Table<T> {
    const WRITEBACK_TABLE: bindings::iomap_writeback_ops = bindings::iomap_writeback_ops {
        writeback_range: Some(Self::writeback_range_callback),
        writeback_submit: Some(Self::writeback_submit_callback),
    };

    extern "C" fn writeback_range_callback(
        wpc: *mut bindings::iomap_writepage_ctx,
        folio_ptr: *mut bindings::folio,
        pos: u64,
        len: u32,
        end_pos: u64,
    ) -> isize {
        from_result(|| {
            pr_info!("writeback_range()\n");

            // SAFETY: For this callback, iomap guarantees:
            //   - `folio_ptr` points to a live `struct folio`.
            //   - The folio remains valid and locked for the duration of this call.
            let address_space_ptr =
                unsafe { (*folio_ptr).__bindgen_anon_1.__bindgen_anon_1.mapping };

            // SAFETY: when iomap_writepages is called, `mapping` is non-NULL and
            // its `host` field points to the owning `struct inode`.
            let inode_ptr = unsafe { (*address_space_ptr).host };

            // SAFETY: `wpc` is guaranteed to be valid.
            // `iomap` is valid storage that we are allowed to mutate.
            let map = unsafe { &mut (*wpc).iomap };

            let ret = Self::iomap_begin_callback(
                inode_ptr,
                pos as Offset,
                len as Offset,
                flags::WRITE,
                map,
                ptr::null_mut(),
            );

            if ret < 0 {
                return Err(Error::from_errno(ret.try_into()?));
            }

            // SAFETY: all arguments provided were provided by the caller
            // This is the go-to method for completing writebacks
            let ret = unsafe {
                bindings::iomap_add_to_ioend(
                    wpc,
                    folio_ptr,
                    pos.try_into()?,
                    end_pos.try_into()?,
                    len,
                )
            };

            if ret < 0 {
                return Err(Error::from_errno(ret.try_into()?));
            }

            Ok(ret)
        })
    }

    extern "C" fn writeback_submit_callback(
        wpc: *mut bindings::iomap_writepage_ctx,
        error: i32,
    ) -> i32 {
        pr_info!("writeback_submit()\n");

        // SAFETY: VFS/iomap guarantees `wpc` is valid here.
        unsafe { bindings::iomap_ioend_writeback_submit(wpc, error) }
    }

    const MAP_TABLE: bindings::iomap_ops = bindings::iomap_ops {
        iomap_begin: Some(Self::iomap_begin_callback),
        iomap_end: Some(Self::iomap_end_callback),
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

            let srcmap_opt: Option<&mut Map<'_>> = if srcmap.is_null() {
                None
            } else {
                // SAFETY: We've just confirmed that srcmap is not null
                Some(unsafe { &mut *srcmap.cast::<Map<'_>>() })
            };

            T::begin(
                inode,
                pos,
                length,
                flags,
                // SAFETY: The C API guarantees that `map` is valid for write.
                unsafe { &mut *map.cast::<Map<'_>>() },
                srcmap_opt,
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
        migrate_folio: Some(bindings::filemap_migrate_folio),
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

    extern "C" fn writepages_callback(
        _mapping: *mut bindings::address_space,
        _wbc: *mut bindings::writeback_control,
    ) -> i32 {
        // Safety: iomap docs say wpc must be zero-initialized.
        let mut wpc: bindings::iomap_writepage_ctx = unsafe { mem::zeroed() };

        // SAFETY: `_mapping` is guaranteed by the VFS to be a valid `struct address_space *`.
        // Its `host` field is a valid `struct inode *`.
        wpc.inode = unsafe { (*_mapping).host };
        wpc.wbc = _wbc;
        wpc.ops = &Self::WRITEBACK_TABLE;

        // SAFETY: We just created `wpc` to be stack-allocated & zero-initialised.
        // The VFS guarantees that `.inode` + `.wbc` are valid while `.ops` is static.
        // We pass a unique mutable pointer to `wpc`.
        unsafe { bindings::iomap_writepages(&mut wpc) }
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

/// Returns address space oprerations backed by iomaps.
pub const fn aops<T: Operations + ?Sized>() -> address_space::Ops<T::FileSystem> {
    address_space::Ops(&Table::<T>::TABLE, PhantomData)
}

pub const fn map_table<T: Operations + ?Sized>() -> &'static bindings::iomap_ops {
    return &Table::<T>::MAP_TABLE;
}

pub fn file_buffered_write<FS: Operations + ?Sized>(
    kiocb: Kiocb<'_, <FS::FileSystem as FileSystem>::Data>,
    iov: &mut IovIterSource<'_>,
) -> Result<usize> {
    // SAFETY:
    // - `kiocb.as_raw()` and `iov.as_raw()` are valid as given by the VFS/iov wrapper.
    // - `map_table::<Ops>()` is a static iomap_ops for this filesystem.
    // - The last two args are allowed to be NULL by iomap.
    let ret = unsafe {
        bindings::iomap_file_buffered_write(
            kiocb.as_raw(),
            iov.as_raw(),
            map_table::<FS>(),
            ptr::null(),
            ptr::null_mut(),
        )
    };

    // Returns Err if err is invalid
    // We can't use it for Ok as it doesn't return usize
    to_result(ret.try_into()?)?;

    ret.try_into().map_err(|_| EOVERFLOW)
}
