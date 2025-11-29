// SPDX-License-Identifier: GPL-2.0

//! Kernel file systems.
//!
//! C headers: [`include/linux/fs.h`](srctree/include/linux/fs.h)

use pin_init::{pin_data, pinned_drop, PinInit, PinnedDrop};

use crate::{
    bindings, dentry,
    error::{from_result, to_result, Error, Result},
    inode::{self, INode},
    mem_cache::MemCache,
    prelude::*,
    sb::{self, SuperBlock},
    str::CStr,
    try_pin_init,
    types::{ForeignOwnable, Opaque},
    ThisModule,
};
use core::{
    marker::{Send, Sync},
    mem::ManuallyDrop,
    pin::Pin,
    ptr,
};

pub mod file;
pub use self::file::{File, LocalFile};

mod kiocb;
pub use self::kiocb::Kiocb;

/// The offset of a file in a file system.
///
/// This is C's `loff_t`.
pub type Offset = i64;

/// An index into the page cache.
///
/// This is C's `pgoff_t`.
pub type PageOffset = usize;

/// Contains constants related to Linux file modes.
pub mod mode {
    /// A bitmask used to the file type from a mode value.
    pub const S_IFMT: u32 = bindings::S_IFMT;

    /// File type constant for block devices.
    pub const S_IFBLK: u32 = bindings::S_IFBLK;

    /// File type constant for char devices.
    pub const S_IFCHR: u32 = bindings::S_IFCHR;

    /// File type constant for directories.
    pub const S_IFDIR: u32 = bindings::S_IFDIR;

    /// File type constant for pipes.
    pub const S_IFIFO: u32 = bindings::S_IFIFO;

    /// File type constant for symbolic links.
    pub const S_IFLNK: u32 = bindings::S_IFLNK;

    /// File type constant for regular files.
    pub const S_IFREG: u32 = bindings::S_IFREG;

    /// File type constant for sockets.
    pub const S_IFSOCK: u32 = bindings::S_IFSOCK;
}

/// A file system type.
pub trait FileSystem {
    /// Data associated with each file system instance (super-block).
    type Data: ForeignOwnable + Send + Sync;

    /// Type of data associated with each inode.
    type INodeData: Send + Sync;

    /// The name of the file system type.
    const NAME: &'static CStr;

    /// Determines how superblocks for this file system type are keyed.
    const SUPER_TYPE: sb::Type = sb::Type::Independent;

    /// Determines if an implementation doesn't specify the required types.
    ///
    /// This is meant for internal use only.
    #[doc(hidden)]
    const IS_UNSPECIFIED: bool = false;

    fn fill_super(
        sb: &mut SuperBlock<Self, sb::New>,
        mapper: Option<inode::Mapper<Self>>, //TODO: Default type parameter should be UnspecifiedFS
    ) -> Result<Self::Data>;

    /// Initialises and returns the root inode of the given superblock.
    ///
    /// This is called during initialisation of a superblock after [`FileSystem::fill_super`] has
    /// completed successfully.
    fn init_root(sb: &SuperBlock<Self>) -> Result<dentry::Root<Self>>;
}

/// A file system that is unspecified.
///
/// Attempting to get super-block or inode data from it will result in a build error.
pub struct UnspecifiedFS;

impl FileSystem for UnspecifiedFS {
    type Data = ();
    type INodeData = ();
    const NAME: &'static CStr = crate::c_str!("unspecified");
    const IS_UNSPECIFIED: bool = true;
    fn fill_super(_: &mut SuperBlock<Self, sb::New>, _: Option<inode::Mapper>) -> Result {
        Err(ENOTSUPP)
    }

    fn init_root(_: &SuperBlock<Self>) -> Result<dentry::Root<Self>> {
        Err(ENOTSUPP)
    }
}

/// A file system registration.
#[pin_data(PinnedDrop)]
pub struct Registration {
    #[pin]
    pub(crate) fs: Opaque<bindings::file_system_type>,
    pub(crate) inode_cache: Option<MemCache>,
}

impl Registration {
    /// Creates a new file system registration.
    ///
    /// It is not visible or accessible yet. A successful call to [`Registration::new`] needs
    /// to be made before users can mount it.
    pub fn new<T: FileSystem + ?Sized>(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            inode_cache: INode::<T>::new_cache()?,
            fs <- Opaque::try_ffi_init(|fs_ptr: *mut bindings::file_system_type| {
                // SAFETY: `try_ffi_init` guarantees that `fs_ptr` is valid for write.
                unsafe { fs_ptr.write(bindings::file_system_type::default()) };

                let fs = unsafe { &mut *fs_ptr };
                fs.owner = module.0;
                fs.name = T::NAME.as_char_ptr();
                fs.init_fs_context = Some(Self::init_fs_context_callback::<T>);
                fs.kill_sb = Some(Self::kill_sb_callback::<T>);
                fs.fs_flags = if let sb::Type::BlockDev = T::SUPER_TYPE {
                    bindings::FS_REQUIRES_DEV as i32
                } else { 0 };

                // SAFETY: Pointers stored in `fs` are static so will live for as long as the
                // registration is active (it is undone in `drop`).
                to_result(unsafe { bindings::register_filesystem(fs_ptr) })
            }),
        })
    }

    unsafe extern "C" fn init_fs_context_callback<T: FileSystem + ?Sized>(
        fc_ptr: *mut bindings::fs_context,
    ) -> ffi::c_int {
        from_result(|| {
            // SAFETY: The C callback API guarantees that `fc_ptr` is valid.
            let fc = unsafe { &mut *fc_ptr };
            fc.ops = &Tables::<T>::CONTEXT;
            Ok(0)
        })
    }

    unsafe extern "C" fn kill_sb_callback<T: FileSystem + ?Sized>(
        sb_ptr: *mut bindings::super_block,
    ) {
        match T::SUPER_TYPE {
            // SAFETY: In `get_tree_callback` we always call `get_tree_bdev` for
            // `sb::Type::BlockDev`, so `kill_block_super` is the appropriate function to call
            // for cleanup.
            sb::Type::BlockDev => unsafe {
                bindings::kill_block_super(sb_ptr);
            },
            // SAFETY: In `get_tree_callback` we always call `get_tree_nodev` for
            // `sb::Type::Independent`, so `kill_anon_super` is the appropriate function to call
            // for cleanup.
            sb::Type::Independent => unsafe {
                bindings::kill_anon_super(sb_ptr);
            },
        }

        let ptr = unsafe { (*sb_ptr).s_fs_info };
        if !ptr.is_null() {
            // SAFETY: The only place where `s_fs_info` is assigned is `NewSuperBlock::init`, where
            // it's initialised with the result of an `into_foreign` call. We checked above that
            // `ptr` is non-null because it would be null if we never reached the point where we
            // init the field.
            unsafe { T::Data::from_foreign(ptr) };
        }
    }
}

// SAFETY: `Registration` doesn't really provide any `&self` methods, so it is safe to pass
// references to it around.
unsafe impl Sync for Registration {}

// SAFETY: Both registration and unregistration are implemented in C and safe to be performed from
// any thread, so `Registration` is `Send`.
unsafe impl Send for Registration {}

#[pinned_drop]
impl PinnedDrop for Registration {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: If an instance of `Self` has been successfully created, a call to
        // `register_filesystem` has necessarily succeeded. So it's ok to call
        // `unregister_filesystem` on the previously registered fs.
        unsafe { bindings::unregister_filesystem(self.fs.get()) };
    }
}

struct Tables<T: FileSystem + ?Sized>(T);
impl<T: FileSystem + ?Sized> Tables<T> {
    const CONTEXT: bindings::fs_context_operations = bindings::fs_context_operations {
        free: None,
        parse_param: None,
        get_tree: Some(Self::get_tree_callback),
        reconfigure: None,
        parse_monolithic: None,
        dup: None,
    };

    unsafe extern "C" fn get_tree_callback(fc: *mut bindings::fs_context) -> ffi::c_int {
        match T::SUPER_TYPE {
            sb::Type::BlockDev => unsafe {
                bindings::get_tree_bdev(fc, Some(Self::fill_super_callback))
            },
            sb::Type::Independent => unsafe {
                bindings::get_tree_nodev(fc, Some(Self::fill_super_callback))
            },
        }
    }

    unsafe extern "C" fn fill_super_callback(
        sb_ptr: *mut bindings::super_block,
        _fc: *mut bindings::fs_context,
    ) -> ffi::c_int {
        from_result(|| {
            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created superblock.
            let new_sb = unsafe { SuperBlock::from_raw_mut(sb_ptr) };

            // SAFETY: The callback contract guarantees that `sb_ptr`, from which `new_sb` is
            // derived, is valid for write.
            let sb = unsafe { &mut *new_sb.0.get() };
            sb.s_op = &Tables::<T>::SUPER_BLOCK;

            let mapper = if matches!(T::SUPER_TYPE, sb::Type::BlockDev) {
                // SAFETY: This is the only mapper created for this inode, so it is unique.
                Some(unsafe { new_sb.bdev().inode().mapper() })
            } else {
                None
            };

            let data = T::fill_super(new_sb, mapper)?;

            // N.B.: Even on failure, `kill_sb` is called and frees the data.
            sb.s_fs_info = data.into_foreign();

            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created (and initialised above) superblock. And we have just initialised
            // `s_fs_info`.
            let sb = unsafe { SuperBlock::from_raw(sb_ptr) };
            let root = T::init_root(sb)?;

            // Reject root inode if it belongs to a different superblock.
            if !ptr::eq(root.super_block(), sb) {
                return Err(EINVAL);
            }

            let dentry = ManuallyDrop::new(root).0.get();

            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created (and initialised above) superblock.
            unsafe { (*sb_ptr).s_root = dentry };

            Ok(0)
        })
    }

    const SUPER_BLOCK: bindings::super_operations = bindings::super_operations {
        alloc_inode: if size_of::<T::INodeData>() != 0 {
            Some(INode::<T>::alloc_inode_callback)
        } else {
            None
        },
        destroy_inode: Some(INode::<T>::destroy_inode_callback),
        free_inode: None,
        dirty_inode: None,
        write_inode: None,
        drop_inode: None,
        evict_inode: None,
        put_super: None,
        sync_fs: None,
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
}
