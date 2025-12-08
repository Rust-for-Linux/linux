use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit};

use bindings::{init_user_ns, kgid_t, kuid_t};

use crate::address_space;
use crate::dentry::{self, DEntry};
use crate::error::{from_err_ptr, from_result, Result};
use crate::folio::{self, Folio};
use crate::fs::{self, mode, Registration};
use crate::fs::{file, PageOffset, UnspecifiedFS};
use crate::mem_cache::MemCache;
use crate::prelude::*;
use crate::sb::SuperBlock;
use crate::str::CString;
use crate::time::Timespec;
use crate::types::{AlwaysRefCounted, Lockable, Locked};
use crate::{block, container_of, inode};
use crate::{
    fs::{FileSystem, Offset},
    time,
    types::{ARef, Opaque},
};
use core::ptr;

pub type Ino = usize;

pub enum INodeState<T: FileSystem + ?Sized> {
    Existing(ARef<INode<T>>),
    Uninitilized(New<T>),
}

/// Operations implemented by inodes.
#[vtable]
pub trait Operations {
    /// File system that these operations are compatible with.
    type FileSystem: FileSystem + ?Sized;

    /// Returns the string that represents the name of the file a symbolic link inode points to.
    ///
    /// When `dentry` is `None`, `get_link` is called with the RCU read-side lock held, so it may
    /// not sleep. Implementations must return `Err(ECHILD)` for it to be called again without
    /// holding the RCU lock.
    fn get_link<'a>(
        _dentry: Option<&DEntry<Self::FileSystem>>,
        _inode: &'a INode<Self::FileSystem>,
    ) -> Result<CString> {
        Err(ENOTSUPP)
    }

    /// Returns the inode corresponding to the directory entry with the given name
    fn lookup(
        _parent: &Locked<&INode<Self::FileSystem>, ReadSem>,
        _dentry: dentry::Unhashed<'_, Self::FileSystem>,
    ) -> Result<Option<ARef<DEntry<Self::FileSystem>>>> {
        Err(ENOTSUPP)
    }

    fn create(
        _parent: &Locked<&INode<Self::FileSystem>, ReadSem>,
        _dentry: dentry::Unhashed<'_, Self::FileSystem>,
        _mode: u16,
        _excl: bool,
    ) -> Result<usize> {
        Err(ENOTSUPP)
    }

    fn unlink(
        _parent: &Locked<&INode<Self::FileSystem>, ReadSem>,
        _dentry: dentry::Unhashed<'_, Self::FileSystem>,
    ) -> Result<usize> {
        Err(ENOTSUPP)
    }
}

/// A node (inode) in the file index.
///
/// Wraps the kernel's `struct inode`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `ihold` ensures that the
/// allocation remains valid at least until the matching call to `iput`.
#[repr(transparent)]
pub struct INode<T: FileSystem + ?Sized = UnspecifiedFS>(
    pub(crate) Opaque<bindings::inode>,
    PhantomData<T>,
);

impl<T: FileSystem + ?Sized> INode<T> {
    /// Creates a new inode reference from the given raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    ///
    /// * `ptr` is valid and remains so for the lifetime of the returned object.
    /// * `ptr` has the correct file system type, or `T` is [`super::UnspecifiedFS`].
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::inode) -> &'a Self {
        // SAFETY: The safety requirements guarantee that the cast below is ok.
        unsafe { &*ptr.cast::<Self>() }
    }

    pub(crate) const fn as_raw(&self) -> *const bindings::inode {
        unsafe { self.0.get() as *const bindings::inode }
    }

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
        unsafe { SuperBlock::from_raw((*self.0.get()).i_sb) }
    }

    pub fn size(&self) -> i64 {
        unsafe { (*self.0.get()).i_size }
    }

    pub fn blocks(&self) -> u64 {
        // SAFETY: this is ok
        unsafe { (*self.0.get()).i_blocks }
    }

    pub fn nlink(&self) -> u32 {
        // SAFETY: this is ok
        unsafe { (*self.0.get()).__bindgen_anon_1.i_nlink }
    }

    pub fn ctime(&self) -> Result<Timespec> {
        let sec = unsafe { (*self.0.get()).i_ctime_sec };
        let nsec = unsafe { (*self.0.get()).i_ctime_nsec };

        Timespec::new(sec.try_into()?, nsec)
    }

    pub fn mtime(&self) -> Result<Timespec> {
        let sec = unsafe { (*self.0.get()).i_mtime_sec };
        let nsec = unsafe { (*self.0.get()).i_mtime_nsec };

        Timespec::new(sec.try_into()?, nsec)
    }

    pub fn atime(&self) -> Result<Timespec> {
        let sec = unsafe { (*self.0.get()).i_atime_sec };
        let nsec = unsafe { (*self.0.get()).i_atime_nsec };

        Timespec::new(sec.try_into()?, nsec)
    }

    pub fn uid(&self) -> u32 {
        let uid = unsafe { (*self.0.get()).i_uid };

        uid.val
    }

    pub fn gid(&self) -> u32 {
        let gid = unsafe { (*self.0.get()).i_gid };

        gid.val
    }

    pub fn mode(&self) -> u16 {
        unsafe { (*self.0.get()).i_mode }
    }

    pub fn truncate_inode_pages_final(&self) {
        // SAFETY: type semantics guarentee that Inode is instatiated
        let data = unsafe { ptr::addr_of_mut!((*self.0.get()).i_data) };
        unsafe { bindings::truncate_inode_pages_final(data) }
    }

    // FIXME: should consume self so you can't call any methods after clearing
    pub fn clear(&self) {
        // SAFETY: type semantics guarentee that Inode is instatiated
        let inode_ptr = self.0.get();
        unsafe { bindings::clear_inode(inode_ptr) }
    }

    // FIXME: Does this work???
    pub unsafe fn set_blocks(&self, num_blocks: u64) {
        unsafe { (*self.0.get()).i_blocks = num_blocks };
    }

    /// Returns the data associated with the inode.
    pub fn data(&self) -> &T::INodeData {
        if T::IS_UNSPECIFIED {
            crate::build_error!("inode data type is unspecified");
        }
        // TODO: Add safety
        let outerp = unsafe { container_of!(self.0.get(), WithData<T::INodeData>, inode) };
        // SAFETY: `self` is guaranteed to be valid by the existence of a shared reference
        // (`&self`) to it. Additionally, we know `T::INodeData` is always initialised in an
        // `INode`.
        unsafe { &*(*outerp).data.as_ptr() }
    }

    /// Returns a mutable reference to the inode's associated data.
    ///
    /// # Safety
    ///
    /// - Callers must ensure exclusive access to this inode's data.
    ///   Typically this means holding the appropriate inode or fs-level lock
    ///   so that no other references (including shared ones) are being used
    ///   concurrently.
    /// - No other references obtained via [`INode::data`] may be used while
    ///   the returned `&mut T::INodeData` is alive.
    pub unsafe fn data_mut(&self) -> &mut T::INodeData {
        if T::IS_UNSPECIFIED {
            crate::build_error!("inode data type is unspecified");
        }
        // TODO: Add safety
        let outerp = unsafe { container_of!(self.0.get(), WithData<T::INodeData>, inode) };
        // SAFETY: `self` is guaranteed to be valid by the existence of a shared reference
        // (`&self`) to it. Additionally, we know `T::INodeData` is always initialised in an
        // `INode`.
        unsafe { &mut *(*outerp).data.as_mut_ptr() }
    }

    pub fn drop_nlink(&self) {
        let inode_ptr = self.0.get();
        // SAFETY: Inode ptr is guaranteed to be valid and instantiated do to the typestate
        unsafe {
            bindings::drop_nlink(inode_ptr);
        }
    }

    /// Returns a mapper for this inode.
    ///
    /// # Safety
    ///
    /// Callers must ensure that mappers are unique for a given inode and range. For inodes that
    /// back a block device, a mapper is always created when the filesystem is mounted; so callers
    /// in such situations must ensure that that mapper is never used.
    pub unsafe fn mapper(&self) -> Mapper<T> {
        Mapper {
            inode: self.into(),
            begin: 0,
            end: Offset::MAX,
        }
    }

    /// Returns a mapped folio at the given offset.
    ///
    /// # Safety
    ///
    /// Callers must ensure that there are no concurrent mutable mappings of the folio.
    pub unsafe fn mapped_folio(
        &self,
        offset: Offset,
    ) -> Result<folio::Mapped<'_, folio::PageCache<T>>> {
        let page_index = offset >> bindings::PAGE_SHIFT;
        let page_offset = offset & ((bindings::PAGE_SIZE - 1) as Offset);
        let folio = self.read_mapping_folio(page_index.try_into()?)?;

        // SAFETY: The safety requirements guarantee that there are no concurrent mutable mappings
        // of the folio.
        unsafe { Folio::map_owned(folio, page_offset.try_into()?) }
    }

    /// Returns the folio at the given page index
    pub fn read_mapping_folio(
        &self,
        index: PageOffset,
    ) -> Result<ARef<Folio<folio::PageCache<T>>>> {
        let folio = from_err_ptr(unsafe {
            bindings::read_mapping_folio(
                (*self.0.get()).i_mapping,
                index.try_into()?,
                ptr::null_mut(),
            )
        })?;
        let ptr = ptr::NonNull::new(folio)
            .ok_or(EIO)?
            .cast::<Folio<folio::PageCache<T>>>();

        // SAFETY: The folio returned by read_mapping_folio has had its refcount incremented.
        Ok(unsafe { ARef::from_raw(ptr) })
    }

    pub(crate) fn new_cache() -> Result<Option<MemCache>> {
        Ok(if size_of::<T::INodeData>() == 0 {
            None
        } else {
            Some(MemCache::try_new::<WithData<T::INodeData>>(
                T::NAME,
                Some(Self::inode_init_once_callback),
            )?)
        })
    }

    pub fn mark_dirty(&self) {
        // SAFETY: This is safe since it is guaranteed by the typestate
        // that the inode has been inserted into the hash
        let inode = self.0.get();
        unsafe { bindings::mark_inode_dirty(inode) };
    }

    unsafe extern "C" fn inode_init_once_callback(outer_inode: *mut core::ffi::c_void) {
        let ptr = outer_inode.cast::<WithData<T::INodeData>>();

        // SAFETY: This is only used in `new`, so we know that we have a valid `inode::WithData`
        // instance whose inode part can be initialised.
        unsafe { bindings::inode_init_once(ptr::addr_of_mut!((*ptr).inode)) };
    }

    pub(crate) unsafe extern "C" fn alloc_inode_callback(
        sb: *mut bindings::super_block,
    ) -> *mut bindings::inode {
        // SAFETY: The callback contract guarantees that `sb` is valid for read.
        let raw_super_type = unsafe { (*sb).s_type };
        let super_type = raw_super_type.cast::<Opaque<bindings::file_system_type>>();

        // SAFETY: This callback is only used in `Registration`, so `super_type` is necessarily
        // embedded in a `Registration`, which is guaranteed to be valid because it has a
        // superblock associated to it.
        let reg = unsafe { &*container_of!(super_type, Registration, fs) };

        // SAFETY: `sb` and `cache` are guaranteed to be valid by the callback contract and by
        // the existence of a superblock respectively.
        let ptr = unsafe {
            bindings::alloc_inode_sb(sb, MemCache::ptr(&reg.inode_cache), bindings::GFP_KERNEL)
        }
        .cast::<WithData<T::INodeData>>();
        if ptr.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: `ptr` was just allocated, so it is valid for dereferencing.
        unsafe { ptr::addr_of_mut!((*ptr).inode) }
    }

    pub(crate) unsafe extern "C" fn destroy_inode_callback(inode: *mut bindings::inode) {
        // SAFETY: By the C contract, `inode` is a valid pointer.
        let is_bad = unsafe { bindings::is_bad_inode(inode) };

        // SAFETY: The inode is guaranteed to be valid by the callback contract. Additionally, the
        // superblock is also guaranteed to still be valid by the inode existence.
        let raw_super_type = unsafe { (*(*inode).i_sb).s_type };
        let super_type = raw_super_type.cast::<Opaque<bindings::file_system_type>>();

        // SAFETY: This callback is only used in `Registration`, so `super_type` is necessarily
        // embedded in a `Registration`, which is guaranteed to be valid because it has a
        // superblock associated to it.
        let reg = unsafe { &*container_of!(super_type, Registration, fs) };
        let ptr = unsafe { container_of!(inode, WithData<T::INodeData>, inode) };

        if !is_bad {
            // SAFETY: The API contract guarantees that `inode` is valid.
            // TODO: Add Link support
            // if unsafe { (*inode).i_mode & mode::S_IFMT == mode::S_IFLNK } {
            //     // SAFETY: We just checked that the inode is a link.
            //     let lnk = unsafe { (*inode).__bindgen_anon_5.i_link };
            //     if !lnk.is_null() {
            //         // SAFETY: This value is on link inode are only populated from with the result
            //         // of `CString::into_foreign`.
            //         unsafe { CString::from_foreign(lnk.cast::<core::ffi::c_void>()) };
            //     }
            // }

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
            unsafe {
                bindings::kmem_cache_free(
                    MemCache::ptr(&reg.inode_cache),
                    ptr.cast::<core::ffi::c_void>(),
                )
            };
        }
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
        unsafe { bindings::iput(obj.as_ref().0.get()) }
    }
}

/// Indicates that the an inode's rw semapahore is locked in read (shared) mode.
pub struct ReadSem;

unsafe impl<T: FileSystem + ?Sized> Lockable<ReadSem> for INode<T> {
    fn raw_lock(&self) {
        // SAFETY: Since there's a reference to the inode, it must be valid.
        unsafe { bindings::inode_lock_shared(self.0.get()) }
    }

    unsafe fn unlock(&self) {
        // SAFETY: Since there's a reference to the inode, it must be valid. Additionally, the
        // safety requirements of this functino require that the inode be locked in read mode.
        unsafe { bindings::inode_unlock_shared(self.0.get()) }
    }
}

/// Allows mapping the contents of the inode.
///
/// # Invariants
///
/// Mappers are unique per range per inode.
pub struct Mapper<T: FileSystem + ?Sized = UnspecifiedFS> {
    inode: ARef<INode<T>>,
    begin: Offset,
    end: Offset,
}

// SAFETY: All inode and folio operations are safe from any thread.
unsafe impl<T: FileSystem + ?Sized> Send for Mapper<T> {}

// SAFETY: All inode and folio operations are safe from any thread.
unsafe impl<T: FileSystem + ?Sized> Sync for Mapper<T> {}

impl<T: FileSystem + ?Sized> Mapper<T> {
    /// Returns a mapped folio at the given offset.
    pub fn mapped_folio(&self, offset: Offset) -> Result<folio::Mapped<'_, folio::PageCache<T>>> {
        if offset < self.begin || offset >= self.end {
            return Err(ERANGE);
        }

        // SAFETY: By the type invariant, there are no other mutable mappings of the folio.
        let mut map = unsafe { self.inode.mapped_folio(offset) }?;
        map.cap_len((self.end - offset).try_into()?);
        Ok(map)
    }

    pub fn read_mapping_folio(
        &self,
        offset: Offset,
    ) -> Result<ARef<folio::Folio<folio::PageCache<T>>>> {
        if offset < self.begin || offset >= self.end {
            return Err(ERANGE);
        }

        let page_index = offset >> bindings::PAGE_SHIFT;
        let mut folio = self.inode.read_mapping_folio(page_index.try_into()?);
        folio
    }
}

struct WithData<T> {
    data: MaybeUninit<T>,
    inode: bindings::inode,
}

/// An inode that is locked and hasn't been initialised yet.
///
/// # Invariants
///
/// The inode is a new one, locked, and valid for write.
pub struct New<T: FileSystem + ?Sized>(
    pub(crate) ptr::NonNull<bindings::inode>,
    pub(crate) PhantomData<T>,
);

impl<T: FileSystem + ?Sized> New<T> {
    /// Initialises the new inode with the given parameters.
    pub fn init_from_disk(self, params: Params<T::INodeData>) -> Result<ARef<INode<T>>> {
        // SAFETY: WithData has been allocated by VFS (allocate_inode_callback)
        let outerp = unsafe { container_of!(self.0.as_ptr(), WithData<T::INodeData>, inode) };

        // SAFETY: This is a newly-created inode. No other references to it exist, so it is
        // safe to mutably dereference it.
        let outer = unsafe { &mut *outerp };

        // N.B. We must always write this to a newly allocated inode because the free callback
        // expects the data to be initialised and drops it.
        outer.data.write(params.value);

        let inode = &mut outer.inode;
        let mode = match params.typ {
            Type::Dir => bindings::S_IFDIR,
            Type::Reg => {
                // SAFETY: The `i_mapping` pointer doesn't change and is valid.
                unsafe { bindings::mapping_set_large_folios(inode.i_mapping) };
                bindings::S_IFREG
            }
            Type::Lnk(str) => {
                // If we are using `page_get_link`, we need to prevent the use of high mem.
                if !inode.i_op.is_null() {
                    // SAFETY: We just checked that `i_op` is non-null, and we always just set it
                    // to valid values.
                    if unsafe {
                        (*inode.i_op).get_link == bindings::page_symlink_inode_operations.get_link
                    } {
                        // SAFETY: `inode` is valid for write as it's a new inode.
                        unsafe { bindings::inode_nohighmem(inode) };
                    }
                }
                // TODO: Look into this
                // if let Some(s) = str {
                //     inode.__bindgen_anon_5.i_link = s.into_foreign().cast::<i8>().cast_mut();
                // }
                bindings::S_IFLNK
            }
            Type::Fifo => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFIFO as _, 0) };
                bindings::S_IFIFO
            }
            Type::Sock => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFSOCK as _, 0) };
                bindings::S_IFSOCK
            }
            Type::Chr(major, minor) => {
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
            Type::Blk(major, minor) => {
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

        inode.i_ctime_sec = params.ctime.tv_sec();
        inode.i_ctime_nsec = params.ctime.tv_nsec()?;
        inode.i_mtime_sec = params.mtime.tv_sec();
        inode.i_mtime_nsec = params.mtime.tv_nsec()?;
        inode.i_atime_sec = params.atime.tv_sec();
        inode.i_atime_nsec = params.atime.tv_nsec()?;

        // SAFETY: inode is a new inode, so it is valid for write.
        unsafe {
            bindings::set_nlink(inode, params.nlink);
            bindings::i_uid_write(inode, params.uid);
            bindings::i_gid_write(inode, params.gid);
            bindings::unlock_new_inode(inode);
        }

        let manual = ManuallyDrop::new(self);
        // SAFETY: We transferred ownership of the refcount to `ARef` by preventing `drop` from
        // being called with the `ManuallyDrop` instance created above.
        Ok(unsafe { ARef::from_raw(manual.0.cast::<INode<T>>()) })
    }

    // Instantiated new inode with data but keep it locked
    pub fn init_new(self, params: Params<T::INodeData>) -> Result<Ready<T>> {
        // SAFETY: WithData has been allocated by VFS (allocate_inode_callback)
        let outerp = unsafe { container_of!(self.0.as_ptr(), WithData<T::INodeData>, inode) };

        // SAFETY: This is a newly-created inode. No other references to it exist, so it is
        // safe to mutably dereference it.
        let outer = unsafe { &mut *outerp };

        // N.B. We must always write this to a newly allocated inode because the free callback
        // expects the data to be initialised and drops it.
        outer.data.write(params.value);

        let inode = &mut outer.inode;
        let mode = match params.typ {
            Type::Dir => bindings::S_IFDIR,
            Type::Reg => {
                // SAFETY: The `i_mapping` pointer doesn't change and is valid.
                unsafe { bindings::mapping_set_large_folios(inode.i_mapping) };
                bindings::S_IFREG
            }
            Type::Lnk(str) => {
                // If we are using `page_get_link`, we need to prevent the use of high mem.
                if !inode.i_op.is_null() {
                    // SAFETY: We just checked that `i_op` is non-null, and we always just set it
                    // to valid values.
                    if unsafe {
                        (*inode.i_op).get_link == bindings::page_symlink_inode_operations.get_link
                    } {
                        // SAFETY: `inode` is valid for write as it's a new inode.
                        unsafe { bindings::inode_nohighmem(inode) };
                    }
                }
                // TODO: Look into this
                // if let Some(s) = str {
                //     inode.__bindgen_anon_5.i_link = s.into_foreign().cast::<i8>().cast_mut();
                // }
                bindings::S_IFLNK
            }
            Type::Fifo => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFIFO as _, 0) };
                bindings::S_IFIFO
            }
            Type::Sock => {
                // SAFETY: `inode` is valid for write as it's a new inode.
                unsafe { bindings::init_special_inode(inode, bindings::S_IFSOCK as _, 0) };
                bindings::S_IFSOCK
            }
            Type::Chr(major, minor) => {
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
            Type::Blk(major, minor) => {
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

        inode.i_ctime_sec = params.ctime.tv_sec();
        inode.i_ctime_nsec = params.ctime.tv_nsec()?;
        inode.i_mtime_sec = params.mtime.tv_sec();
        inode.i_mtime_nsec = params.mtime.tv_nsec()?;
        inode.i_atime_sec = params.atime.tv_sec();
        inode.i_atime_nsec = params.atime.tv_nsec()?;

        // SAFETY: inode is a new inode, so it is valid for write.
        unsafe {
            bindings::set_nlink(inode, params.nlink);
            bindings::i_uid_write(inode, params.uid);
            bindings::i_gid_write(inode, params.gid);
        }

        // SAFETY: inode is new and I_NEW is set, insert into hash
        let ret = unsafe { bindings::insert_inode_locked(inode) };
        if ret != 0 {
            return Err(EINVAL);
        }

        let manual = ManuallyDrop::new(self);

        Ok(Ready(manual.0, PhantomData))
    }

    pub fn set_iops(&mut self, iops: Ops<T>) -> &mut Self {
        let inode = unsafe { self.0.as_mut() };
        inode.i_op = iops.0;
        self
    }

    /// Sets the file operations on this new inode.
    pub fn set_fops(&mut self, fops: file::Ops<T>) -> &mut Self {
        // SAFETY: By the type invariants, it's ok to modify the inode.
        let inode = unsafe { self.0.as_mut() };
        inode.__bindgen_anon_3.i_fop = fops.inner;
        self
    }

    /// Sets the address space operations on this new inode.
    pub fn set_aops(&mut self, aops: address_space::Ops<T>) -> &mut Self {
        // SAFETY: By the type invariants, it's ok to modify the inode.
        let inode = unsafe { self.0.as_mut() };
        inode.i_data.a_ops = aops.0;
        self
    }

    pub fn set_ino(&mut self, ino: Ino) -> &mut Self {
        // SAFETY: By the type invariants, it's ok to modify the inode.
        let inode = unsafe { self.0.as_mut() };
        inode.i_ino = ino;
        self
    }

    // Initilize uid and gid of a new inode and return (assoicated with a new file)
    pub fn init_owner(
        &mut self,
        parent: &Locked<&INode<T>, kernel::inode::ReadSem>,
        mode: u16,
    ) -> (kuid_t, kgid_t) {
        let inode = unsafe { self.0.as_mut() };
        let parent = unsafe { parent.as_raw() };

        unsafe {
            bindings::inode_init_owner(&raw mut bindings::nop_mnt_idmap, inode, parent, mode);
        }

        (inode.i_uid, inode.i_gid)
    }
}

impl<T: FileSystem + ?Sized> Drop for New<T> {
    fn drop(&mut self) {
        // SAFETY: The new inode failed to be turned into an initialised inode, so it's safe (and
        // in fact required) to call `iget_failed` on it.
        unsafe { bindings::iget_failed(self.0.as_ptr()) };
    }
}

/// An inode that is locked has been initilised but
/// needs to be inserted into hash and linked with dentry
///
/// # Invariants
/// The inode is a new one, locked, and instantiated
pub struct Ready<T: FileSystem + ?Sized>(
    pub(crate) ptr::NonNull<bindings::inode>,
    pub(crate) PhantomData<T>,
);

impl<T: FileSystem + ?Sized> Ready<T> {
    pub fn mark_dirty(&mut self) {
        // SAFETY: This is safe since it is guaranteed by the typestate
        // that the inode has been inserted into the hash
        let inode = unsafe { self.0.as_mut() };
        unsafe { bindings::mark_inode_dirty(inode) };
    }

    pub fn instantiate_dentry(self, dentry: &dentry::Unhashed<'_, T>) {
        let inode_ptr = self.0.as_ptr();
        let dentry_ptr = dentry.0 .0.get();

        // SAFETY: instantiates dentry and unlocks inode
        // transfer ownership to C
        unsafe {
            bindings::d_instantiate_new(dentry_ptr, inode_ptr);
        }

        core::mem::forget(self);
    }
    /// Returns the number of the inode.
    pub fn ino(&self) -> Ino {
        // SAFETY: `i_ino` is immutable, and `self` is guaranteed to be valid by the existence of a
        // shared reference (&self) to it.
        unsafe { (*self.0.as_ref()).i_ino }
    }
}

/// The type of an inode.
pub enum Type {
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
    Lnk(Option<CString>),

    /// Named unix-domain socket type.
    Sock,
}

/// Required inode parameters.
///
/// This is used when creating new inodes.
pub struct Params<T> {
    /// The access mode. It's a mask that grants execute (1), write (2) and read (4) access to
    /// everyone, the owner group, and the owner.
    pub mode: u16,

    /// Type of inode.
    ///
    /// Also carries additional per-type data.
    pub typ: Type,

    /// Size of the contents of the inode.
    ///
    /// Its maximum value is [`super::MAX_LFS_FILESIZE`].
    pub size: Offset,

    /// Number of blocks.
    pub blocks: block::Count,

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

/// Represents inode operations.
pub struct Ops<T: FileSystem + ?Sized>(*const bindings::inode_operations, PhantomData<T>);

impl<T: FileSystem + ?Sized> Ops<T> {
    /// Returns inode operations for symbolic links that are stored in a single page.
    pub fn page_symlink_inode() -> Self {
        // SAFETY: This is a constant in C, it never changes.
        Self(
            unsafe { &bindings::page_symlink_inode_operations },
            PhantomData,
        )
    }

    /// Returns inode operations for symbolic links that are stored in the `i_lnk` field.
    pub fn simple_symlink_inode() -> Self {
        // SAFETY: This is a constant in C, it never changes.
        Self(
            unsafe { &bindings::simple_symlink_inode_operations },
            PhantomData,
        )
    }

    /// Creates the inode operations from a type that implements the [`Operations`] trait.
    pub const fn new<U: Operations<FileSystem = T> + ?Sized>() -> Self {
        struct Table<T: Operations + ?Sized>(PhantomData<T>);
        impl<T: Operations + ?Sized> Table<T> {
            const TABLE: bindings::inode_operations = bindings::inode_operations {
                lookup: if T::HAS_LOOKUP {
                    Some(Self::lookup_callback)
                } else {
                    None
                },
                get_link: None,
                // get_link: if T::HAS_GET_LINK {
                //     Some(Self::get_link_callback)
                // } else {
                //     None
                // },
                permission: None,
                get_inode_acl: None,
                readlink: None,
                create: if T::HAS_CREATE {
                    Some(Self::create_callback)
                } else {
                    None
                },
                link: None,
                unlink: if T::HAS_UNLINK {
                    Some(Self::unlink_callback)
                } else {
                    None
                },
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
                dentry_ptr: *mut bindings::dentry,
                _flags: u32,
            ) -> *mut bindings::dentry {
                // SAFETY: The C API guarantees that `parent_ptr` is a valid inode.
                let parent = unsafe { INode::from_raw(parent_ptr) };

                // SAFETY: The C API guarantees that `dentry_ptr` is a valid dentry.
                let dentry = unsafe { DEntry::from_raw(dentry_ptr) };

                // SAFETY: The C API guarantees that the inode's rw semaphore is locked at least in
                // read mode. It does not expect callees to unlock it, so we make the locked object
                // manually dropped to avoid unlocking it.
                let locked = ManuallyDrop::new(unsafe { Locked::new(parent) });

                match T::lookup(&locked, dentry::Unhashed(dentry)) {
                    Err(e) => e.to_ptr(),
                    Ok(None) => ptr::null_mut(),
                    Ok(Some(ret)) => ManuallyDrop::new(ret).0.get(),
                }
            }

            // TODO: add mnt_idmap support
            unsafe extern "C" fn create_callback(
                _mnt_idmap_ptr: *mut bindings::mnt_idmap,
                parent_ptr: *mut bindings::inode,
                dentry_ptr: *mut bindings::dentry,
                mode: u16,
                excl: bool,
            ) -> i32 {
                from_result(|| {
                    // SAFETY: The C API guarantees that `parent_ptr` is a valid inode.
                    let parent = unsafe { INode::from_raw(parent_ptr) };

                    // SAFETY: The C API guarantees that `parent_ptr` is a valid inode.
                    let dentry = unsafe { DEntry::from_raw(dentry_ptr) };

                    // SAFETY: The C API guarantees that the inode's rw semaphore is locked at least in
                    // read mode. It does not expect callees to unlock it, so we make the locked object
                    // manually dropped to avoid unlocking it.
                    let locked = ManuallyDrop::new(unsafe { Locked::new(parent) });

                    let create = T::create(&locked, dentry::Unhashed(dentry), mode, excl)?;
                    Ok(i32::try_from(create)?)
                })
            }

            unsafe extern "C" fn unlink_callback(
                inode_ptr: *mut bindings::inode,
                dentry_ptr: *mut bindings::dentry,
            ) -> i32 {
                from_result(|| {
                    // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                    let inode = unsafe { INode::from_raw(inode_ptr) };

                    // SAFETY: The C API guarantees that `dentry_ptr` is a valid inode.
                    let dentry = unsafe { DEntry::from_raw(dentry_ptr) };

                    // SAFETY: The C API guarantees that the inode's rw semaphore is locked at least in
                    // read mode. It does not expect callees to unlock it, so we make the locked object
                    // manually dropped to avoid unlocking it.
                    let locked = ManuallyDrop::new(unsafe { Locked::new(inode) });

                    let unlink = T::unlink(&locked, dentry::Unhashed(dentry))?;

                    Ok(i32::try_from(unlink)?)
                })
            }
        }
        Self(&Table::<U>::TABLE, PhantomData)
    }
}
