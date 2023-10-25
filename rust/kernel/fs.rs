// SPDX-License-Identifier: GPL-2.0

//! Kernel file systems.
//!
//! This module allows Rust code to register new kernel file systems.
//!
//! C headers: [`include/linux/fs.h`](../../include/linux/fs.h)

use crate::error::{code::*, from_result, to_result, Error, Result};
use crate::folio::{LockedFolio, UniqueFolio};
use crate::types::{ARef, AlwaysRefCounted, Either, ForeignOwnable, Opaque, ScopeGuard};
use crate::{
    bindings, container_of, init::PinInit, mem_cache::MemCache, str::CStr, time::Timespec,
    try_pin_init, ThisModule,
};
use core::mem::{size_of, ManuallyDrop, MaybeUninit};
use core::{marker::PhantomData, marker::PhantomPinned, pin::Pin, ptr};
use macros::{pin_data, pinned_drop};

#[cfg(CONFIG_BUFFER_HEAD)]
pub mod buffer;

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

/// Maximum size of an inode.
pub const MAX_LFS_FILESIZE: i64 = bindings::MAX_LFS_FILESIZE;

/// Type of superblock keying.
///
/// It determines how C's `fs_context_operations::get_tree` is implemented.
pub enum Super {
    /// Multiple independent superblocks may exist.
    Independent,

    /// Uses a block device.
    BlockDev,
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
    const SUPER_TYPE: Super = Super::Independent;

    /// Returns the parameters to initialise a super block.
    fn super_params(sb: &NewSuperBlock<Self>) -> Result<SuperParams<Self::Data>>;

    /// Initialises and returns the root inode of the given superblock.
    ///
    /// This is called during initialisation of a superblock after [`FileSystem::super_params`] has
    /// completed successfully.
    fn init_root(sb: &SuperBlock<Self>) -> Result<ARef<INode<Self>>>;

    /// Reads directory entries from directory inodes.
    ///
    /// [`DirEmitter::pos`] holds the current position of the directory reader.
    fn read_dir(inode: &INode<Self>, emitter: &mut DirEmitter) -> Result;

    /// Returns the inode corresponding to the directory entry with the given name.
    fn lookup(parent: &INode<Self>, name: &[u8]) -> Result<ARef<INode<Self>>>;

    /// Reads the contents of the inode into the given folio.
    fn read_folio(inode: &INode<Self>, folio: LockedFolio<'_>) -> Result;

    /// Reads an xattr.
    ///
    /// Returns the number of bytes written to `outbuf`. If it is too small, returns the number of
    /// bytes needs to hold the attribute.
    fn read_xattr(_inode: &INode<Self>, _name: &CStr, _outbuf: &mut [u8]) -> Result<usize> {
        Err(EOPNOTSUPP)
    }

    /// Get filesystem statistics.
    fn statfs(_sb: &SuperBlock<Self>) -> Result<Stat> {
        Err(ENOSYS)
    }
}

/// File system stats.
///
/// A subset of C's `kstatfs`.
pub struct Stat {
    /// Magic number of the file system.
    pub magic: u32,

    /// The maximum length of a file name.
    pub namelen: i64,

    /// Block size.
    pub bsize: i64,

    /// Number of files in the file system.
    pub files: u64,

    /// Number of blocks in the file system.
    pub blocks: u64,
}

/// The types of directory entries reported by [`FileSystem::read_dir`].
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum DirEntryType {
    /// Unknown type.
    Unknown = bindings::DT_UNKNOWN,

    /// Named pipe (first-in, first-out) type.
    Fifo = bindings::DT_FIFO,

    /// Character device type.
    Chr = bindings::DT_CHR,

    /// Directory type.
    Dir = bindings::DT_DIR,

    /// Block device type.
    Blk = bindings::DT_BLK,

    /// Regular file type.
    Reg = bindings::DT_REG,

    /// Symbolic link type.
    Lnk = bindings::DT_LNK,

    /// Named unix-domain socket type.
    Sock = bindings::DT_SOCK,

    /// White-out type.
    Wht = bindings::DT_WHT,
}

impl From<INodeType> for DirEntryType {
    fn from(value: INodeType) -> Self {
        match value {
            INodeType::Fifo => DirEntryType::Fifo,
            INodeType::Chr(_, _) => DirEntryType::Chr,
            INodeType::Dir => DirEntryType::Dir,
            INodeType::Blk(_, _) => DirEntryType::Blk,
            INodeType::Reg => DirEntryType::Reg,
            INodeType::Lnk => DirEntryType::Lnk,
            INodeType::Sock => DirEntryType::Sock,
        }
    }
}

impl core::convert::TryFrom<u32> for DirEntryType {
    type Error = crate::error::Error;

    fn try_from(v: u32) -> Result<Self> {
        match v {
            v if v == Self::Unknown as u32 => Ok(Self::Unknown),
            v if v == Self::Fifo as u32 => Ok(Self::Fifo),
            v if v == Self::Chr as u32 => Ok(Self::Chr),
            v if v == Self::Dir as u32 => Ok(Self::Dir),
            v if v == Self::Blk as u32 => Ok(Self::Blk),
            v if v == Self::Reg as u32 => Ok(Self::Reg),
            v if v == Self::Lnk as u32 => Ok(Self::Lnk),
            v if v == Self::Sock as u32 => Ok(Self::Sock),
            v if v == Self::Wht as u32 => Ok(Self::Wht),
            _ => Err(EDOM),
        }
    }
}

/// A registration of a file system.
#[pin_data(PinnedDrop)]
pub struct Registration {
    #[pin]
    fs: Opaque<bindings::file_system_type>,
    inode_cache: Option<MemCache>,
    #[pin]
    _pin: PhantomPinned,
}

// SAFETY: `Registration` doesn't provide any `&self` methods, so it is safe to pass references
// to it around.
unsafe impl Sync for Registration {}

// SAFETY: Both registration and unregistration are implemented in C and safe to be performed
// from any thread, so `Registration` is `Send`.
unsafe impl Send for Registration {}

impl Registration {
    /// Creates the initialiser of a new file system registration.
    pub fn new<T: FileSystem + ?Sized>(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            _pin: PhantomPinned,
            inode_cache: if size_of::<T::INodeData>() == 0 {
                None
            } else {
                Some(MemCache::try_new::<INodeWithData<T::INodeData>>(
                    T::NAME,
                    Some(Self::inode_init_once_callback::<T>),
                )?)
            },
            fs <- Opaque::try_ffi_init(|fs_ptr: *mut bindings::file_system_type| {
                // SAFETY: `try_ffi_init` guarantees that `fs_ptr` is valid for write.
                unsafe { fs_ptr.write(bindings::file_system_type::default()) };

                // SAFETY: `try_ffi_init` guarantees that `fs_ptr` is valid for write, and it has
                // just been initialised above, so it's also valid for read.
                let fs = unsafe { &mut *fs_ptr };
                fs.owner = module.0;
                fs.name = T::NAME.as_char_ptr();
                fs.init_fs_context = Some(Self::init_fs_context_callback::<T>);
                fs.kill_sb = Some(Self::kill_sb_callback::<T>);
                fs.fs_flags = if let Super::BlockDev = T::SUPER_TYPE {
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
    ) -> core::ffi::c_int {
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
            // `Super::BlockDev`, so `kill_block_super` is the appropriate function to call
            // for cleanup.
            Super::BlockDev => unsafe { bindings::kill_block_super(sb_ptr) },
            // SAFETY: In `get_tree_callback` we always call `get_tree_nodev` for
            // `Super::Independent`, so `kill_anon_super` is the appropriate function to call
            // for cleanup.
            Super::Independent => unsafe { bindings::kill_anon_super(sb_ptr) },
        }

        // SAFETY: The C API contract guarantees that `sb_ptr` is valid for read.
        let ptr = unsafe { (*sb_ptr).s_fs_info };
        if !ptr.is_null() {
            // SAFETY: The only place where `s_fs_info` is assigned is `NewSuperBlock::init`, where
            // it's initialised with the result of an `into_foreign` call. We checked above that
            // `ptr` is non-null because it would be null if we never reached the point where we
            // init the field.
            unsafe { T::Data::from_foreign(ptr) };
        }
    }

    unsafe extern "C" fn inode_init_once_callback<T: FileSystem + ?Sized>(
        outer_inode: *mut core::ffi::c_void,
    ) {
        let ptr = outer_inode.cast::<INodeWithData<T::INodeData>>();

        // SAFETY: This is only used in `new`, so we know that we have a valid `INodeWithData`
        // instance whose inode part can be initialised.
        unsafe { bindings::inode_init_once(ptr::addr_of_mut!((*ptr).inode)) };
    }
}

#[pinned_drop]
impl PinnedDrop for Registration {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: If an instance of `Self` has been successfully created, a call to
        // `register_filesystem` has necessarily succeeded. So it's ok to call
        // `unregister_filesystem` on the previously registered fs.
        unsafe { bindings::unregister_filesystem(self.fs.get()) };
    }
}

/// The number of an inode.
pub type Ino = u64;

/// A node in the file system index (inode).
///
/// Wraps the kernel's `struct inode`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `ihold` ensures that the
/// allocation remains valid at least until the matching call to `iput`.
#[repr(transparent)]
pub struct INode<T: FileSystem + ?Sized>(Opaque<bindings::inode>, PhantomData<T>);

impl<T: FileSystem + ?Sized> INode<T> {
    /// Returns the number of the inode.
    pub fn ino(&self) -> Ino {
        // SAFETY: `i_ino` is immutable, and `self` is guaranteed to be valid by the existence of a
        // shared reference (&self) to it.
        unsafe { (*self.0.get()).i_ino }
    }

    /// Returns the super-block that owns the inode.
    pub fn super_block(&self) -> &SuperBlock<T> {
        // SAFETY: `i_sb` is immutable, and `self` is guaranteed to be valid by the existence of a
        // shared reference (&self) to it.
        unsafe { &*(*self.0.get()).i_sb.cast() }
    }

    /// Returns the data associated with the inode.
    pub fn data(&self) -> &T::INodeData {
        let outerp = container_of!(self.0.get(), INodeWithData<T::INodeData>, inode);
        // SAFETY: `self` is guaranteed to be valid by the existence of a shared reference
        // (`&self`) to it. Additionally, we know `T::INodeData` is always initialised in an
        // `INode`.
        unsafe { &*(*outerp).data.as_ptr() }
    }

    /// Returns the size of the inode contents.
    pub fn size(&self) -> i64 {
        // SAFETY: `self` is guaranteed to be valid by the existence of a shared reference.
        unsafe { bindings::i_size_read(self.0.get()) }
    }
}

// SAFETY: The type invariants guarantee that `INode` is always ref-counted.
unsafe impl<T: FileSystem + ?Sized> AlwaysRefCounted for INode<T> {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::ihold(self.0.get()) };
    }

    unsafe fn dec_ref(obj: ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::iput(obj.cast().as_ptr()) }
    }
}

struct INodeWithData<T> {
    data: MaybeUninit<T>,
    inode: bindings::inode,
}

/// An inode that is locked and hasn't been initialised yet.
#[repr(transparent)]
pub struct NewINode<T: FileSystem + ?Sized>(ARef<INode<T>>);

impl<T: FileSystem + ?Sized> NewINode<T> {
    /// Initialises the new inode with the given parameters.
    pub fn init(self, params: INodeParams<T::INodeData>) -> Result<ARef<INode<T>>> {
        let outerp = container_of!(self.0 .0.get(), INodeWithData<T::INodeData>, inode);

        // SAFETY: This is a newly-created inode. No other references to it exist, so it is
        // safe to mutably dereference it.
        let outer = unsafe { &mut *outerp.cast_mut() };

        // N.B. We must always write this to a newly allocated inode because the free callback
        // expects the data to be initialised and drops it.
        outer.data.write(params.value);

        let inode = &mut outer.inode;

        let mode = match params.typ {
            INodeType::Dir => {
                inode.__bindgen_anon_3.i_fop = &Tables::<T>::DIR_FILE_OPERATIONS;
                inode.i_op = &Tables::<T>::DIR_INODE_OPERATIONS;
                bindings::S_IFDIR
            }
            INodeType::Reg => {
                // SAFETY: `generic_ro_fops` never changes, it's safe to reference it.
                inode.__bindgen_anon_3.i_fop = unsafe { &bindings::generic_ro_fops };
                inode.i_data.a_ops = &Tables::<T>::FILE_ADDRESS_SPACE_OPERATIONS;

                // SAFETY: The `i_mapping` pointer doesn't change and is valid.
                unsafe { bindings::mapping_set_large_folios(inode.i_mapping) };
                bindings::S_IFREG
            }
            INodeType::Lnk => {
                inode.i_op = &Tables::<T>::LNK_INODE_OPERATIONS;
                inode.i_data.a_ops = &Tables::<T>::FILE_ADDRESS_SPACE_OPERATIONS;

                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::inode_nohighmem(inode) };
                bindings::S_IFLNK
            }
            INodeType::Fifo => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFIFO as _, 0) };
                bindings::S_IFIFO
            }
            INodeType::Sock => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFSOCK as _, 0) };
                bindings::S_IFSOCK
            }
            INodeType::Chr(major, minor) => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe {
                    bindings::init_special_inode(
                        inode,
                        bindings::S_IFCHR as _,
                        bindings::MKDEV(major, minor & bindings::MINORMASK),
                    )
                };
                bindings::S_IFCHR
            }
            INodeType::Blk(major, minor) => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe {
                    bindings::init_special_inode(
                        inode,
                        bindings::S_IFBLK as _,
                        bindings::MKDEV(major, minor & bindings::MINORMASK),
                    )
                };
                bindings::S_IFBLK
            }
        };

        inode.i_mode = (params.mode & 0o777) | u16::try_from(mode)?;
        inode.i_size = params.size;
        inode.i_blocks = params.blocks;

        inode.__i_ctime = params.ctime.into();
        inode.i_mtime = params.mtime.into();
        inode.i_atime = params.atime.into();

        // SAFETY: inode is a new inode, so it is valid for write.
        unsafe {
            bindings::set_nlink(inode, params.nlink);
            bindings::i_uid_write(inode, params.uid);
            bindings::i_gid_write(inode, params.gid);
            bindings::unlock_new_inode(inode);
        }

        // SAFETY: We are manually destructuring `self` and preventing `drop` from being called.
        Ok(unsafe { (&ManuallyDrop::new(self).0 as *const ARef<INode<T>>).read() })
    }
}

impl<T: FileSystem + ?Sized> Drop for NewINode<T> {
    fn drop(&mut self) {
        // SAFETY: The new inode failed to be turned into an initialised inode, so it's safe (and
        // in fact required) to call `iget_failed` on it.
        unsafe { bindings::iget_failed(self.0 .0.get()) };
    }
}

/// The type of the inode.
#[derive(Copy, Clone)]
pub enum INodeType {
    /// Named pipe (first-in, first-out) type.
    Fifo,

    /// Character device type.
    Chr(u32, u32),

    /// Directory type.
    Dir,

    /// Block device type.
    Blk(u32, u32),

    /// Regular file type.
    Reg,

    /// Symbolic link type.
    Lnk,

    /// Named unix-domain socket type.
    Sock,
}

/// Required inode parameters.
///
/// This is used when creating new inodes.
pub struct INodeParams<T> {
    /// The access mode. It's a mask that grants execute (1), write (2) and read (4) access to
    /// everyone, the owner group, and the owner.
    pub mode: u16,

    /// Type of inode.
    ///
    /// Also carries additional per-type data.
    pub typ: INodeType,

    /// Size of the contents of the inode.
    ///
    /// Its maximum value is [`MAX_LFS_FILESIZE`].
    pub size: i64,

    /// Number of blocks.
    pub blocks: u64,

    /// Number of links to the inode.
    pub nlink: u32,

    /// User id.
    pub uid: u32,

    /// Group id.
    pub gid: u32,

    /// Creation time.
    pub ctime: Timespec,

    /// Last modification time.
    pub mtime: Timespec,

    /// Last access time.
    pub atime: Timespec,

    /// Value to attach to this node.
    pub value: T,
}

/// A file system super block.
///
/// Wraps the kernel's `struct super_block`.
#[repr(transparent)]
pub struct SuperBlock<T: FileSystem + ?Sized>(Opaque<bindings::super_block>, PhantomData<T>);

impl<T: FileSystem + ?Sized> SuperBlock<T> {
    /// Returns the data associated with the superblock.
    pub fn data(&self) -> <T::Data as ForeignOwnable>::Borrowed<'_> {
        // SAFETY: This method is only available after the `NeedsData` typestate, so `s_fs_info`
        // has been initialised initialised with the result of a call to `T::into_foreign`.
        let ptr = unsafe { (*self.0.get()).s_fs_info };
        unsafe { T::Data::borrow(ptr) }
    }

    /// Tries to get an existing inode or create a new one if it doesn't exist yet.
    pub fn get_or_create_inode(&self, ino: Ino) -> Result<Either<ARef<INode<T>>, NewINode<T>>> {
        // SAFETY: The only initialisation missing from the superblock is the root, and this
        // function is needed to create the root, so it's safe to call it.
        let inode =
            ptr::NonNull::new(unsafe { bindings::iget_locked(self.0.get(), ino) }).ok_or(ENOMEM)?;

        // SAFETY: `inode` is valid for read, but there could be concurrent writers (e.g., if it's
        // an already-initialised inode), so we use `read_volatile` to read its current state.
        let state = unsafe { ptr::read_volatile(ptr::addr_of!((*inode.as_ptr()).i_state)) };
        if state & u64::from(bindings::I_NEW) == 0 {
            // The inode is cached. Just return it.
            //
            // SAFETY: `inode` had its refcount incremented by `iget_locked`; this increment is now
            // owned by `ARef`.
            Ok(Either::Left(unsafe { ARef::from_raw(inode.cast()) }))
        } else {
            // SAFETY: The new inode is valid but not fully initialised yet, so it's ok to create a
            // `NewINode`.
            Ok(Either::Right(NewINode(unsafe {
                ARef::from_raw(inode.cast())
            })))
        }
    }

    /// Reads a block from the block device.
    #[cfg(CONFIG_BUFFER_HEAD)]
    pub fn bread(&self, block: u64) -> Result<ARef<buffer::Head>> {
        // Fail requests for non-blockdev file systems. This is a compile-time check.
        match T::SUPER_TYPE {
            Super::BlockDev => {}
            _ => return Err(EIO),
        }

        // SAFETY: This function is only valid after the `NeedsInit` typestate, so the block size
        // is known and the superblock can be used to read blocks.
        let ptr =
            ptr::NonNull::new(unsafe { bindings::sb_bread(self.0.get(), block) }).ok_or(EIO)?;
        // SAFETY: `sb_bread` returns a referenced buffer head. Ownership of the increment is
        // passed to the `ARef` instance.
        Ok(unsafe { ARef::from_raw(ptr.cast()) })
    }

    /// Reads `size` bytes starting from `offset` bytes.
    ///
    /// Returns an iterator that returns slices based on blocks.
    #[cfg(CONFIG_BUFFER_HEAD)]
    pub fn read(
        &self,
        offset: u64,
        size: u64,
    ) -> Result<impl Iterator<Item = Result<buffer::View>> + '_> {
        struct BlockIter<'a, T: FileSystem + ?Sized> {
            sb: &'a SuperBlock<T>,
            next_offset: u64,
            end: u64,
        }
        impl<'a, T: FileSystem + ?Sized> Iterator for BlockIter<'a, T> {
            type Item = Result<buffer::View>;

            fn next(&mut self) -> Option<Self::Item> {
                if self.next_offset >= self.end {
                    return None;
                }

                // SAFETY: The superblock is valid and has had its block size initialised.
                let block_size = unsafe { (*self.sb.0.get()).s_blocksize };
                let bh = match self.sb.bread(self.next_offset / block_size) {
                    Ok(bh) => bh,
                    Err(e) => return Some(Err(e)),
                };
                let boffset = self.next_offset & (block_size - 1);
                let bsize = core::cmp::min(self.end - self.next_offset, block_size - boffset);
                self.next_offset += bsize;
                Some(Ok(buffer::View::new(bh, boffset as usize, bsize as usize)))
            }
        }
        Ok(BlockIter {
            sb: self,
            next_offset: offset,
            end: offset.checked_add(size).ok_or(ERANGE)?,
        })
    }
}

/// Required superblock parameters.
///
/// This is returned by implementations of [`FileSystem::super_params`].
pub struct SuperParams<T: ForeignOwnable + Send + Sync> {
    /// The magic number of the superblock.
    pub magic: u32,

    /// The size of a block in powers of 2 (i.e., for a value of `n`, the size is `2^n`).
    pub blocksize_bits: u8,

    /// Maximum size of a file.
    ///
    /// The maximum allowed value is [`MAX_LFS_FILESIZE`].
    pub maxbytes: i64,

    /// Granularity of c/m/atime in ns (cannot be worse than a second).
    pub time_gran: u32,

    /// Data to be associated with the superblock.
    pub data: T,
}

/// A superblock that is still being initialised.
///
/// # Invariants
///
/// The superblock is a newly-created one and this is the only active pointer to it.
#[repr(transparent)]
pub struct NewSuperBlock<T: FileSystem + ?Sized>(bindings::super_block, PhantomData<T>);

impl<T: FileSystem + ?Sized> NewSuperBlock<T> {
    /// Reads sectors.
    ///
    /// `count` must be such that the total size doesn't exceed a page.
    pub fn sread(&self, sector: u64, count: usize, folio: &mut UniqueFolio) -> Result {
        // Fail requests for non-blockdev file systems. This is a compile-time check.
        match T::SUPER_TYPE {
            // The superblock is valid and given that it's a blockdev superblock it must have a
            // valid `s_bdev`.
            Super::BlockDev => {}
            _ => return Err(EIO),
        }

        crate::build_assert!(count * (bindings::SECTOR_SIZE as usize) <= bindings::PAGE_SIZE);

        // Read the sectors.
        let mut bio = bindings::bio::default();
        let bvec = Opaque::<bindings::bio_vec>::uninit();

        // SAFETY: `bio` and `bvec` are allocated on the stack, they're both valid.
        unsafe {
            bindings::bio_init(
                &mut bio,
                self.0.s_bdev,
                bvec.get(),
                1,
                bindings::req_op_REQ_OP_READ,
            )
        };

        // SAFETY: `bio` was just initialised with `bio_init` above, so it's safe to call
        // `bio_uninit` on the way out.
        let mut bio =
            ScopeGuard::new_with_data(bio, |mut b| unsafe { bindings::bio_uninit(&mut b) });

        // SAFETY: We have one free `bvec` (initialsied above). We also know that size won't exceed
        // a page size (build_assert above).
        unsafe {
            bindings::bio_add_folio_nofail(
                &mut *bio,
                folio.0 .0.get(),
                count * (bindings::SECTOR_SIZE as usize),
                0,
            )
        };
        bio.bi_iter.bi_sector = sector;

        // SAFETY: The bio was fully initialised above.
        to_result(unsafe { bindings::submit_bio_wait(&mut *bio) })?;
        Ok(())
    }

    /// Returns the number of sectors in the underlying block device.
    pub fn sector_count(&self) -> Result<u64> {
        // Fail requests for non-blockdev file systems. This is a compile-time check.
        match T::SUPER_TYPE {
            // The superblock is valid and given that it's a blockdev superblock it must have a
            // valid `s_bdev`.
            Super::BlockDev => Ok(unsafe { bindings::bdev_nr_sectors(self.0.s_bdev) }),
            _ => Err(EIO),
        }
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

    unsafe extern "C" fn get_tree_callback(fc: *mut bindings::fs_context) -> core::ffi::c_int {
        match T::SUPER_TYPE {
            // SAFETY: `fc` is valid per the callback contract. `fill_super_callback` also has
            // the right type and is a valid callback.
            Super::BlockDev => unsafe {
                bindings::get_tree_bdev(fc, Some(Self::fill_super_callback))
            },
            // SAFETY: `fc` is valid per the callback contract. `fill_super_callback` also has
            // the right type and is a valid callback.
            Super::Independent => unsafe {
                bindings::get_tree_nodev(fc, Some(Self::fill_super_callback))
            },
        }
    }

    unsafe extern "C" fn fill_super_callback(
        sb_ptr: *mut bindings::super_block,
        _fc: *mut bindings::fs_context,
    ) -> core::ffi::c_int {
        from_result(|| {
            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created superblock.
            let sb = unsafe { &mut *sb_ptr.cast() };
            let params = T::super_params(sb)?;

            sb.0.s_magic = params.magic as _;
            sb.0.s_op = &Tables::<T>::SUPER_BLOCK;
            sb.0.s_xattr = &Tables::<T>::XATTR_HANDLERS[0];
            sb.0.s_maxbytes = params.maxbytes;
            sb.0.s_time_gran = params.time_gran;
            sb.0.s_blocksize_bits = params.blocksize_bits;
            sb.0.s_blocksize = 1;
            if sb.0.s_blocksize.leading_zeros() < params.blocksize_bits.into() {
                return Err(EINVAL);
            }
            sb.0.s_blocksize = 1 << sb.0.s_blocksize_bits;
            sb.0.s_flags |= bindings::SB_RDONLY;

            // N.B.: Even on failure, `kill_sb` is called and frees the data.
            sb.0.s_fs_info = params.data.into_foreign().cast_mut();

            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created (and initialised above) superblock.
            let sb = unsafe { &mut *sb_ptr.cast() };
            let root = T::init_root(sb)?;

            // Reject root inode if it belongs to a different superblock.
            if !ptr::eq(root.super_block(), sb) {
                return Err(EINVAL);
            }

            // SAFETY: `d_make_root` requires that `inode` be valid and referenced, which is the
            // case for this call.
            //
            // It takes over the inode, even on failure, so we don't need to clean it up.
            let dentry = unsafe { bindings::d_make_root(ManuallyDrop::new(root).0.get()) };
            if dentry.is_null() {
                return Err(ENOMEM);
            }

            // SAFETY: The callback contract guarantees that `sb_ptr` is a unique pointer to a
            // newly-created (and initialised above) superblock.
            unsafe { (*sb_ptr).s_root = dentry };

            Ok(0)
        })
    }

    const SUPER_BLOCK: bindings::super_operations = bindings::super_operations {
        alloc_inode: if size_of::<T::INodeData>() != 0 {
            Some(Self::alloc_inode_callback)
        } else {
            None
        },
        destroy_inode: Some(Self::destroy_inode_callback),
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
        statfs: Some(Self::statfs_callback),
        remount_fs: None,
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

    unsafe extern "C" fn alloc_inode_callback(
        sb: *mut bindings::super_block,
    ) -> *mut bindings::inode {
        // SAFETY: The callback contract guarantees that `sb` is valid for read.
        let super_type = unsafe { (*sb).s_type };

        // SAFETY: This callback is only used in `Registration`, so `super_type` is necessarily
        // embedded in a `Registration`, which is guaranteed to be valid because it has a
        // superblock associated to it.
        let reg = unsafe { &*container_of!(super_type, Registration, fs) };

        // SAFETY: `sb` and `cache` are guaranteed to be valid by the callback contract and by
        // the existence of a superblock respectively.
        let ptr = unsafe {
            bindings::alloc_inode_sb(sb, MemCache::ptr(&reg.inode_cache), bindings::GFP_KERNEL)
        }
        .cast::<INodeWithData<T::INodeData>>();
        if ptr.is_null() {
            return ptr::null_mut();
        }
        ptr::addr_of_mut!((*ptr).inode)
    }

    unsafe extern "C" fn destroy_inode_callback(inode: *mut bindings::inode) {
        // SAFETY: By the C contract, `inode` is a valid pointer.
        let is_bad = unsafe { bindings::is_bad_inode(inode) };

        // SAFETY: The inode is guaranteed to be valid by the callback contract. Additionally, the
        // superblock is also guaranteed to still be valid by the inode existence.
        let super_type = unsafe { (*(*inode).i_sb).s_type };

        // SAFETY: This callback is only used in `Registration`, so `super_type` is necessarily
        // embedded in a `Registration`, which is guaranteed to be valid because it has a
        // superblock associated to it.
        let reg = unsafe { &*container_of!(super_type, Registration, fs) };
        let ptr = container_of!(inode, INodeWithData<T::INodeData>, inode).cast_mut();

        if !is_bad {
            // SAFETY: The code either initialises the data or marks the inode as bad. Since the
            // inode is not bad, the data is initialised, and thus safe to drop.
            unsafe { ptr::drop_in_place((*ptr).data.as_mut_ptr()) };
        }

        if size_of::<T::INodeData>() == 0 {
            // SAFETY: When the size of `INodeData` is zero, we don't use a separate mem_cache, so
            // it is allocated from the regular mem_cache, which is what `free_inode_nonrcu` uses
            // to free the inode.
            unsafe { bindings::free_inode_nonrcu(inode) };
        } else {
            // The callback contract guarantees that the inode was previously allocated via the
            // `alloc_inode_callback` callback, so it is safe to free it back to the cache.
            unsafe { bindings::kmem_cache_free(MemCache::ptr(&reg.inode_cache), ptr.cast()) };
        }
    }

    unsafe extern "C" fn statfs_callback(
        dentry: *mut bindings::dentry,
        buf: *mut bindings::kstatfs,
    ) -> core::ffi::c_int {
        from_result(|| {
            // SAFETY: The C API guarantees that `dentry` is valid for read. `d_sb` is
            // immutable, so it's safe to read it. The superblock is guaranteed to be valid dor
            // the duration of the call.
            let sb = unsafe { &*(*dentry).d_sb.cast::<SuperBlock<T>>() };
            let s = T::statfs(sb)?;

            // SAFETY: The C API guarantees that `buf` is valid for read and write.
            let buf = unsafe { &mut *buf };
            buf.f_type = s.magic.into();
            buf.f_namelen = s.namelen;
            buf.f_bsize = s.bsize;
            buf.f_files = s.files;
            buf.f_blocks = s.blocks;
            buf.f_bfree = 0;
            buf.f_bavail = 0;
            buf.f_ffree = 0;
            Ok(0)
        })
    }

    const XATTR_HANDLERS: [*const bindings::xattr_handler; 2] = [&Self::XATTR_HANDLER, ptr::null()];

    const XATTR_HANDLER: bindings::xattr_handler = bindings::xattr_handler {
        name: ptr::null(),
        prefix: crate::c_str!("").as_char_ptr(),
        flags: 0,
        list: None,
        get: Some(Self::xattr_get_callback),
        set: None,
    };

    unsafe extern "C" fn xattr_get_callback(
        _handler: *const bindings::xattr_handler,
        _dentry: *mut bindings::dentry,
        inode_ptr: *mut bindings::inode,
        name: *const core::ffi::c_char,
        buffer: *mut core::ffi::c_void,
        size: usize,
    ) -> core::ffi::c_int {
        from_result(|| {
            // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
            let inode = unsafe { &*inode_ptr.cast::<INode<T>>() };

            // SAFETY: The c API guarantees that `name` is a valid null-terminated string. It
            // also guarantees that it's valid for the duration of the callback.
            let name = unsafe { CStr::from_char_ptr(name) };

            // SAFETY: The C API guarantees that `buffer` is at least `size` bytes in length.
            let buf = unsafe { core::slice::from_raw_parts_mut(buffer.cast(), size) };
            let len = T::read_xattr(inode, name, buf)?;
            Ok(len.try_into()?)
        })
    }

    const DIR_FILE_OPERATIONS: bindings::file_operations = bindings::file_operations {
        owner: ptr::null_mut(),
        llseek: Some(bindings::generic_file_llseek),
        read: Some(bindings::generic_read_dir),
        write: None,
        read_iter: None,
        write_iter: None,
        iopoll: None,
        iterate_shared: Some(Self::read_dir_callback),
        poll: None,
        unlocked_ioctl: None,
        compat_ioctl: None,
        mmap: None,
        mmap_supported_flags: 0,
        open: None,
        flush: None,
        release: None,
        fsync: None,
        fasync: None,
        lock: None,
        get_unmapped_area: None,
        check_flags: None,
        flock: None,
        splice_write: None,
        splice_read: None,
        splice_eof: None,
        setlease: None,
        fallocate: None,
        show_fdinfo: None,
        copy_file_range: None,
        remap_file_range: None,
        fadvise: None,
        uring_cmd: None,
        uring_cmd_iopoll: None,
    };

    unsafe extern "C" fn read_dir_callback(
        file: *mut bindings::file,
        ctx_ptr: *mut bindings::dir_context,
    ) -> core::ffi::c_int {
        from_result(|| {
            // SAFETY: The C API guarantees that `file` is valid for read. And since `f_inode` is
            // immutable, we can read it directly.
            let inode = unsafe { &*(*file).f_inode.cast::<INode<T>>() };

            // SAFETY: The C API guarantees that this is the only reference to the `dir_context`
            // instance.
            let emitter = unsafe { &mut *ctx_ptr.cast::<DirEmitter>() };
            let orig_pos = emitter.pos();

            // Call the module implementation. We ignore errors if directory entries have been
            // succesfully emitted: this is because we want users to see them before the error.
            match T::read_dir(inode, emitter) {
                Ok(_) => Ok(0),
                Err(e) => {
                    if emitter.pos() == orig_pos {
                        Err(e)
                    } else {
                        Ok(0)
                    }
                }
            }
        })
    }

    const DIR_INODE_OPERATIONS: bindings::inode_operations = bindings::inode_operations {
        lookup: Some(Self::lookup_callback),
        get_link: None,
        permission: None,
        get_inode_acl: None,
        readlink: None,
        create: None,
        link: None,
        unlink: None,
        symlink: None,
        mkdir: None,
        rmdir: None,
        mknod: None,
        rename: None,
        setattr: None,
        getattr: None,
        listxattr: None,
        fiemap: None,
        update_time: None,
        atomic_open: None,
        tmpfile: None,
        get_acl: None,
        set_acl: None,
        fileattr_set: None,
        fileattr_get: None,
        get_offset_ctx: None,
    };

    extern "C" fn lookup_callback(
        parent_ptr: *mut bindings::inode,
        dentry: *mut bindings::dentry,
        _flags: u32,
    ) -> *mut bindings::dentry {
        // SAFETY: The C API guarantees that `parent_ptr` is a valid inode.
        let parent = unsafe { &*parent_ptr.cast::<INode<T>>() };

        // SAFETY: The C API guarantees that `dentry` is valid for read. Since the name is
        // immutable, it's ok to read its length directly.
        let len = unsafe { (*dentry).d_name.__bindgen_anon_1.__bindgen_anon_1.len };
        let Ok(name_len) = usize::try_from(len) else {
            return ENOENT.to_ptr();
        };

        // SAFETY: The C API guarantees that `dentry` is valid for read. Since the name is
        // immutable, it's ok to read it directly.
        let name = unsafe { core::slice::from_raw_parts((*dentry).d_name.name, name_len) };
        match T::lookup(parent, name) {
            Err(e) => e.to_ptr(),
            // SAFETY: The returned inode is valid and referenced (by the type invariants), so
            // it is ok to transfer this increment to `d_splice_alias`.
            Ok(inode) => unsafe {
                bindings::d_splice_alias(ManuallyDrop::new(inode).0.get(), dentry)
            },
        }
    }

    const LNK_INODE_OPERATIONS: bindings::inode_operations = bindings::inode_operations {
        lookup: None,
        get_link: Some(bindings::page_get_link),
        permission: None,
        get_inode_acl: None,
        readlink: None,
        create: None,
        link: None,
        unlink: None,
        symlink: None,
        mkdir: None,
        rmdir: None,
        mknod: None,
        rename: None,
        setattr: None,
        getattr: None,
        listxattr: None,
        fiemap: None,
        update_time: None,
        atomic_open: None,
        tmpfile: None,
        get_acl: None,
        set_acl: None,
        fileattr_set: None,
        fileattr_get: None,
        get_offset_ctx: None,
    };

    const FILE_ADDRESS_SPACE_OPERATIONS: bindings::address_space_operations =
        bindings::address_space_operations {
            writepage: None,
            read_folio: Some(Self::read_folio_callback),
            writepages: None,
            dirty_folio: None,
            readahead: None,
            write_begin: None,
            write_end: None,
            bmap: None,
            invalidate_folio: None,
            release_folio: None,
            free_folio: None,
            direct_IO: None,
            migrate_folio: None,
            launder_folio: None,
            is_partially_uptodate: None,
            is_dirty_writeback: None,
            error_remove_page: None,
            swap_activate: None,
            swap_deactivate: None,
            swap_rw: None,
        };

    extern "C" fn read_folio_callback(
        _file: *mut bindings::file,
        folio: *mut bindings::folio,
    ) -> i32 {
        from_result(|| {
            // SAFETY: All pointers are valid and stable.
            let inode = unsafe {
                &*(*(*folio)
                    .__bindgen_anon_1
                    .page
                    .__bindgen_anon_1
                    .__bindgen_anon_1
                    .mapping)
                    .host
                    .cast::<INode<T>>()
            };

            // SAFETY: The C contract guarantees that the folio is valid and locked, with ownership
            // of the lock transferred to the callee (this function). The folio is also guaranteed
            // not to outlive this function.
            T::read_folio(inode, unsafe { LockedFolio::from_raw(folio) })?;
            Ok(0)
        })
    }
}

/// Directory entry emitter.
///
/// This is used in [`FileSystem::read_dir`] implementations to report the directory entry.
#[repr(transparent)]
pub struct DirEmitter(bindings::dir_context);

impl DirEmitter {
    /// Returns the current position of the emitter.
    pub fn pos(&self) -> i64 {
        self.0.pos
    }

    /// Emits a directory entry.
    ///
    /// `pos_inc` is the number with which to increment the current position on success.
    ///
    /// `name` is the name of the entry.
    ///
    /// `ino` is the inode number of the entry.
    ///
    /// `etype` is the type of the entry.
    ///
    /// Returns `false` when the entry could not be emitted, possibly because the user-provided
    /// buffer is full.
    pub fn emit(&mut self, pos_inc: i64, name: &[u8], ino: Ino, etype: DirEntryType) -> bool {
        let Ok(name_len) = i32::try_from(name.len()) else {
            return false;
        };

        let Some(actor) = self.0.actor else {
            return false;
        };

        let Some(new_pos) = self.0.pos.checked_add(pos_inc) else {
            return false;
        };

        // SAFETY: `name` is valid at least for the duration of the `actor` call.
        let ret = unsafe {
            actor(
                &mut self.0,
                name.as_ptr().cast(),
                name_len,
                self.0.pos,
                ino,
                etype as _,
            )
        };
        if ret {
            self.0.pos = new_pos;
        }
        ret
    }
}

/// Kernel module that exposes a single file system implemented by `T`.
#[pin_data]
pub struct Module<T: FileSystem + ?Sized> {
    #[pin]
    fs_reg: Registration,
    _p: PhantomData<T>,
}

impl<T: FileSystem + ?Sized + Sync + Send> crate::InPlaceModule for Module<T> {
    fn init(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            fs_reg <- Registration::new::<T>(module),
            _p: PhantomData,
        })
    }
}

/// Declares a kernel module that exposes a single file system.
///
/// The `type` argument must be a type which implements the [`FileSystem`] trait. Also accepts
/// various forms of kernel metadata.
///
/// # Examples
///
/// ```
/// # mod module_fs_sample {
/// use kernel::fs::{DirEmitter, INode, NewSuperBlock, SuperBlock, SuperParams};
/// use kernel::prelude::*;
/// use kernel::{c_str, folio::LockedFolio, fs, types::ARef};
///
/// kernel::module_fs! {
///     type: MyFs,
///     name: "myfs",
///     author: "Rust for Linux Contributors",
///     description: "My Rust fs",
///     license: "GPL",
/// }
///
/// struct MyFs;
/// impl fs::FileSystem for MyFs {
///     type Data = ();
///     type INodeData =();
///     const NAME: &'static CStr = c_str!("myfs");
///     fn super_params(_: &NewSuperBlock<Self>) -> Result<SuperParams<Self::Data>> {
///         todo!()
///     }
///     fn init_root(_sb: &SuperBlock<Self>) -> Result<ARef<INode<Self>>> {
///         todo!()
///     }
///     fn read_dir(_: &INode<Self>, _: &mut DirEmitter) -> Result {
///         todo!()
///     }
///     fn lookup(_: &INode<Self>, _: &[u8]) -> Result<ARef<INode<Self>>> {
///         todo!()
///     }
///     fn read_folio(_: &INode<Self>, _: LockedFolio<'_>) -> Result {
///         todo!()
///     }
/// }
/// # }
/// ```
#[macro_export]
macro_rules! module_fs {
    (type: $type:ty, $($f:tt)*) => {
        type ModuleType = $crate::fs::Module<$type>;
        $crate::macros::module! {
            type: ModuleType,
            $($f)*
        }
    }
}
