use core::{marker::PhantomData, ptr};

use crate::error::{code::*, Result};
use crate::types::{ARef, ForeignOwnable};
use crate::{block, inode};
use crate::{
    build_error,
    fs::FileSystem,
    inode::{INodeState, Ino},
    types::Opaque,
};

pub enum Type {
    /// Multiple independent superblocks may exist.
    Independent,
    /// Uses a block device.
    BlockDev,
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
        let ptr = unsafe { (*self.0.get()).s_fs_info };
        unsafe { T::Data::borrow(ptr) }
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
            // SAFETY: The new inode is valid but not fully initialised yet, so it's ok to create a
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
