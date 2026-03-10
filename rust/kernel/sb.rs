use core::{
    marker::PhantomData,
    ops::Deref,
    ptr::{self, NonNull},
};

use crate::prelude::*;
use crate::types::{ARef, ForeignOwnable};
use crate::{block, inode};
use crate::{
    build_error,
    fs::FileSystem,
    inode::{INodeState, Ino},
    types::Opaque,
};
use crate::{
    error::from_result,
    page::{BorrowedPage, Page},
};
use crate::{
    error::{code::*, from_err_ptr, Result},
    inode::INode,
};

pub enum Type {
    /// Multiple independent superblocks may exist.
    Independent,
    /// Uses a block device.
    BlockDev,
}

/// Operations implemented by super blocks
#[vtable]
pub trait Operations {
    type FileSystem: FileSystem + ?Sized;

    fn evict_inode(_inode: &INode<Self::FileSystem>) -> Result {
        Err(ENOTSUPP)
    }

    fn write_inode(_inode: &INode<Self::FileSystem>) -> Result<usize> {
        Err(ENOTSUPP)
    }

    fn sync_fs(_sb: &SuperBlock<Self::FileSystem>) -> Result<usize> {
        Err(ENOTSUPP)
    }
}

/// Indicates that a superblock in this typestate has data initialized.
///
/// # Safety
///
/// Implementers must ensure that `s_fs_info` is properly initialised in this state.
#[doc(hidden)]
pub unsafe trait DataInited {}

/// A typestate for [`SuperBlock`] that indicates that it's a new one, so not fully initialized
/// yet.
pub enum New {}

/// A typestate for [`SuperBlock`] that indicates that it's ready to be used.
pub enum Ready {}

// SAFETY: Instances of `SuperBlock<T, Ready>` are only created after initialising the data.
unsafe impl DataInited for Ready {}

/// A file system superblock
///
/// Wraps the kernel's `struct super_block`.
#[repr(transparent)]
pub struct SuperBlock<T: FileSystem + ?Sized, S = Ready>(
    pub(crate) Opaque<bindings::super_block>,
    PhantomData<(S, T)>,
);

impl<T: FileSystem + ?Sized, S> SuperBlock<T, S> {
    /// Creates a new superblock reference from the given raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    ///
    /// * `ptr` is valid and remains so for the lifetime of the returned object.
    /// * `ptr` has the correct file system type, or `T` is [`super::UnspecifiedFS`].
    /// * `ptr` in the right typestate.
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::super_block) -> &'a Self {
        // SAFETY: The safety requirements guarantee that the cast below is ok.
        unsafe { &*ptr.cast::<Self>() }
    }

    /// Creates a new superblock mutable reference from the given raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    ///
    /// * `ptr` is valid and remains so for the lifetime of the returned object.
    /// * `ptr` has the correct file system type, or `T` is [`super::UnspecifiedFS`].
    /// * `ptr` in the right typestate.
    /// * `ptr` is the only active pointer to the superblock.
    pub(crate) unsafe fn from_raw_mut<'a>(ptr: *mut bindings::super_block) -> &'a mut Self {
        // SAFETY: The safety requirements guarantee that the cast below is ok.
        unsafe { &mut *ptr.cast::<Self>() }
    }

    pub fn bdev(&self) -> &block::Device {
        if !matches!(T::SUPER_TYPE, Type::BlockDev) {
            build_error!("bdev is only available in blockdev superblocks");
        }

        // SAFETY: The superblock is valid and given that it's a blockdev superblock it must have a
        // valid `s_bdev` that remains valid while the superblock (`self`) is valid.
        unsafe { block::Device::from_raw((*self.0.get()).s_bdev) }
    }

    pub fn blocksize_bits(&self) -> u8 {
        // SAFETY: This should be fine??
        unsafe { (*self.0.get()).s_blocksize_bits }
    }

    pub fn read_mapping_page<'a>(&'a self, index: u64) -> Result<MappingPage<'a>> {
        let bdev = self.bdev();

        // SAFETY: all block devices have a valid bd_mapping
        let mapping = unsafe { (*bdev.0.get()).bd_mapping };

        // SAFETY: mapping is initilized above
        unsafe { MappingPage::read(self, mapping, index) }
    }
}

/// To be used when acquiring pages with read_mapping_page
pub struct MappingPage<'a> {
    page: BorrowedPage<'a>,
}

impl<'a> Deref for MappingPage<'a> {
    type Target = Page;
    fn deref(&self) -> &Self::Target {
        &self.page
    }
}

impl<'a> MappingPage<'a> {
    pub fn as_ptr(&self) -> *mut bindings::page {
        self.page.as_ptr()
    }

    /// `<C>` determines the lifetime of this page reference
    ///
    /// # Safety
    ///
    /// `mapping` must point to a valid reference of `struct address_space`
    pub unsafe fn read<C>(
        _ctx: &'a C,
        mapping: *mut bindings::address_space,
        index: u64,
    ) -> Result<MappingPage<'a>> {
        // SAFETY: given a valid mapping and an index, the VFS will return a valid page
        // or an error pointer
        let page_ptr = from_err_ptr(unsafe {
            bindings::read_mapping_page(mapping, index as usize, ptr::null_mut())
        })?;

        if page_ptr.is_null() {
            return Err(EIO);
        }

        // SAFETY: `vmalloc_to_page` returns a valid pointer to a `struct page` for a valid
        // pointer to `Vmalloc` memory.
        let page = unsafe { NonNull::new_unchecked(page_ptr) };

        // SAFETY: `page` is a valid page
        let borrowed_page = unsafe { BorrowedPage::from_raw(page) };

        Ok(MappingPage {
            page: borrowed_page,
        })
    }
}

impl<'a> Drop for MappingPage<'a> {
    fn drop(&mut self) {
        // SAFETY: `read_mapping_page` gave us a ref; dropping this wrapper
        // means we're done with it, so we drop that ref.
        unsafe { bindings::put_page(self.as_ptr()) };
    }
}

impl<T: FileSystem + ?Sized> SuperBlock<T, New> {
    /// Sets the magic number of the superblock.
    pub fn set_magic(&mut self, magic: usize) -> &mut Self {
        // SAFETY: This is a new superblock that is being initialised, so it's ok to write to its
        // fields.
        unsafe { (*self.0.get()).s_magic = magic };
        self
    }
}

impl<T: FileSystem + ?Sized, S: DataInited> SuperBlock<T, S> {
    /// Returns the data associated with the superblock.
    pub fn data(&self) -> <T::Data as ForeignOwnable>::Borrowed<'_> {
        if T::IS_UNSPECIFIED {
            crate::build_error!("super block data type is unspecified");
        }

        // SAFETY: This method is only available if the typestate implements `DataInited`, whose
        // safety requirements include `s_fs_info` being properly initialised.
        unsafe {
            let ptr = (*self.0.get()).s_fs_info;
            T::Data::borrow(ptr)
        }
    }

    /// Tries to get an existing inode or create a new one if it doesn't exist yet.
    ///
    /// This method is not callable from a superblock where data isn't inited yet because it would
    /// allow one to get access to the uninited data via `inode::New::init()` ->
    /// `INode::super_block()` -> `SuperBlock::data()`.
    pub fn get_or_create_inode(&self, ino: Ino) -> Result<INodeState<T>> {
        // SAFETY: All superblock-related state needed by `iget_locked` is initialised by C code
        // before calling `fill_super_callback`, or by `fill_super_callback` itself before calling
        // `super_params`, which is the first function to see a new superblock.
        let inode =
            ptr::NonNull::new(unsafe { bindings::iget_locked(self.0.get(), ino) }).ok_or(ENOMEM)?;

        // SAFETY: `inode` is a valid pointer returned by `iget_locked`.
        unsafe { bindings::spin_lock(ptr::addr_of_mut!((*inode.as_ptr()).i_lock)) };

        // SAFETY: `inode` is valid and was locked by the previous lock.
        let state = unsafe { *ptr::addr_of!((*inode.as_ptr()).i_state) };

        // SAFETY: `inode` is a valid pointer returned by `iget_locked`.
        unsafe { bindings::spin_unlock(ptr::addr_of_mut!((*inode.as_ptr()).i_lock)) };

        // TODO: investigate if size of state is variable
        // if state & u64::from(bindings::I_NEW) == 0 was old code
        if state & bindings::inode_state_flags_t_I_NEW == 0 {
            // The inode is cached. Just return it.
            //
            // SAFETY: `inode` had its refcount incremented by `iget_locked`; this increment is now
            // owned by `ARef`.
            Ok(INodeState::Existing(unsafe {
                ARef::from_raw(inode.cast())
            }))
        } else {
            // The new inode is valid but not fully initialised yet, so it's ok to create a
            // `inode::New`.
            Ok(INodeState::Uninitilized(inode::New(inode, PhantomData)))
        }
    }

    pub fn new_inode(&self) -> Result<inode::New<T>> {
        let sb_ptr = self.0.get();
        // SAFETY: sb is guaranteed to be valid because of TypeState
        let new_inode = ptr::NonNull::new(unsafe { bindings::new_inode(sb_ptr) }).ok_or(ENOMEM)?;

        Ok(inode::New(new_inode, PhantomData))
    }
}

/// Represents inode operations.
pub struct Ops<T: FileSystem + ?Sized> {
    pub(crate) inner: *const bindings::super_operations,
    _p: PhantomData<T>,
}

impl<T: FileSystem + ?Sized> Ops<T> {
    pub const fn new<U: Operations<FileSystem = T> + ?Sized>() -> Self {
        struct Table<T: Operations + ?Sized>(PhantomData<T>);
        impl<T: Operations + ?Sized> Table<T> {
            const TABLE: bindings::super_operations = bindings::super_operations {
                alloc_inode: if size_of::<<T::FileSystem as FileSystem>::INodeData>() != 0 {
                    Some(INode::<T::FileSystem>::alloc_inode_callback)
                } else {
                    None
                },
                destroy_inode: Some(INode::<T::FileSystem>::destroy_inode_callback),
                free_inode: None,
                dirty_inode: None,
                write_inode: if T::HAS_WRITE_INODE {
                    Some(Self::write_inode_callback)
                } else {
                    None
                },
                drop_inode: None,
                evict_inode: if T::HAS_EVICT_INODE {
                    Some(Self::evict_inode_callback)
                } else {
                    None
                },
                put_super: None,
                sync_fs: if T::HAS_SYNC_FS {
                    Some(Self::sync_fs_callback)
                } else {
                    None
                },
                freeze_super: None,
                freeze_fs: None,
                thaw_super: None,
                unfreeze_fs: None,
                statfs: None,
                remount_fs: None,
                remove_bdev: None, // TODO: New field, research
                umount_begin: None,
                show_options: None,
                show_devname: None,
                show_path: None,
                show_stats: None,
                #[cfg(CONFIG_QUOTA)]
                quota_read: None,
                #[cfg(CONFIG_QUOTA)]
                quota_write: None,
                #[cfg(CONFIG_QUOTA)]
                get_dquots: None,
                nr_cached_objects: None,
                free_cached_objects: None,
                shutdown: None,
            };

            extern "C" fn evict_inode_callback(inode_ptr: *mut bindings::inode) {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };

                T::evict_inode(inode); // TODO: Should this return something?
            }

            extern "C" fn write_inode_callback(
                inode_ptr: *mut bindings::inode,
                _wbc: *mut bindings::writeback_control,
            ) -> i32 {
                // TODO: add support for wbc
                from_result(|| {
                    // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                    let inode = unsafe { INode::from_raw(inode_ptr) };

                    let write = T::write_inode(inode)?;

                    Ok(i32::try_from(write)?)
                })
            }

            extern "C" fn sync_fs_callback(sb_ptr: *mut bindings::super_block, _wait: i32) -> i32 {
                // TODO: add support for wait
                from_result(|| {
                    // SAFETY: The C API guarantees that `sb_ptr` is a valid inode.
                    let sb = unsafe { SuperBlock::from_raw(sb_ptr) };

                    let sync_fs = T::sync_fs(sb)?;

                    Ok(i32::try_from(sync_fs)?)
                })
            }
        }
        Self {
            inner: &Table::<U>::TABLE,
            _p: PhantomData,
        }
    }
}
