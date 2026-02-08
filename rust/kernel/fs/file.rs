// SPDX-License-Identifier: GPL-2.0

// Copyright (C) 2024 Google LLC.

//! Files and file descriptors.
//!
//! C headers: [`include/linux/fs.h`](srctree/include/linux/fs.h) and
//! [`include/linux/file.h`](srctree/include/linux/file.h)

use crate::prelude::*;
use crate::{
    bindings,
    cred::Credential,
    error::{code::*, from_result, to_result, Error, Result},
    fmt,
    fs::{FileSystem, Kiocb, Offset, UnspecifiedFS},
    inode::{self, INode, Ino},
    iov::{IovIterDest, IovIterSource},
    kernel::dentry::DEntry,
    sync::aref::{ARef, AlwaysRefCounted},
    types::{ForeignOwnable, Locked, NotThreadSafe, Opaque},
    user,
};

use core::{marker::PhantomData, mem::ManuallyDrop, ptr};

/// Primitive type representing the offset within a [`File`].
///
/// Type alias for `bindings::loff_t`.
pub type Offset = bindings::loff_t;

/// Flags associated with a [`File`].
pub mod flags {
    /// File is opened in append mode.
    pub const O_APPEND: u32 = bindings::O_APPEND;

    /// Signal-driven I/O is enabled.
    pub const O_ASYNC: u32 = bindings::FASYNC;

    /// Close-on-exec flag is set.
    pub const O_CLOEXEC: u32 = bindings::O_CLOEXEC;

    /// File was created if it didn't already exist.
    pub const O_CREAT: u32 = bindings::O_CREAT;

    /// Direct I/O is enabled for this file.
    pub const O_DIRECT: u32 = bindings::O_DIRECT;

    /// File must be a directory.
    pub const O_DIRECTORY: u32 = bindings::O_DIRECTORY;

    /// Like [`O_SYNC`] except metadata is not synced.
    pub const O_DSYNC: u32 = bindings::O_DSYNC;

    /// Ensure that this file is created with the `open(2)` call.
    pub const O_EXCL: u32 = bindings::O_EXCL;

    /// Large file size enabled (`off64_t` over `off_t`).
    pub const O_LARGEFILE: u32 = bindings::O_LARGEFILE;

    /// Do not update the file last access time.
    pub const O_NOATIME: u32 = bindings::O_NOATIME;

    /// File should not be used as process's controlling terminal.
    pub const O_NOCTTY: u32 = bindings::O_NOCTTY;

    /// If basename of path is a symbolic link, fail open.
    pub const O_NOFOLLOW: u32 = bindings::O_NOFOLLOW;

    /// File is using nonblocking I/O.
    pub const O_NONBLOCK: u32 = bindings::O_NONBLOCK;

    /// File is using nonblocking I/O.
    ///
    /// This is effectively the same flag as [`O_NONBLOCK`] on all architectures
    /// except SPARC64.
    pub const O_NDELAY: u32 = bindings::O_NDELAY;

    /// Used to obtain a path file descriptor.
    pub const O_PATH: u32 = bindings::O_PATH;

    /// Write operations on this file will flush data and metadata.
    pub const O_SYNC: u32 = bindings::O_SYNC;

    /// This file is an unnamed temporary regular file.
    pub const O_TMPFILE: u32 = bindings::O_TMPFILE;

    /// File should be truncated to length 0.
    pub const O_TRUNC: u32 = bindings::O_TRUNC;

    /// Bitmask for access mode flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use kernel::fs::file;
    /// # fn do_something() {}
    /// # let flags = 0;
    /// if (flags & file::flags::O_ACCMODE) == file::flags::O_RDONLY {
    ///     do_something();
    /// }
    /// ```
    pub const O_ACCMODE: u32 = bindings::O_ACCMODE;

    /// File is read only.
    pub const O_RDONLY: u32 = bindings::O_RDONLY;

    /// File is write only.
    pub const O_WRONLY: u32 = bindings::O_WRONLY;

    /// File can be both read and written.
    pub const O_RDWR: u32 = bindings::O_RDWR;
}

/// Wraps the kernel's `struct file`. Thread safe.
///
/// This represents an open file rather than a file on a filesystem. Processes generally reference
/// open files using file descriptors. However, file descriptors are not the same as files. A file
/// descriptor is just an integer that corresponds to a file, and a single file may be referenced
/// by multiple file descriptors.
///
/// # Refcounting
///
/// Instances of this type are reference-counted. The reference count is incremented by the
/// `fget`/`get_file` functions and decremented by `fput`. The Rust type `ARef<File>` represents a
/// pointer that owns a reference count on the file.
///
/// Whenever a process opens a file descriptor (fd), it stores a pointer to the file in its fd
/// table (`struct files_struct`). This pointer owns a reference count to the file, ensuring the
/// file isn't prematurely deleted while the file descriptor is open. In Rust terminology, the
/// pointers in `struct files_struct` are `ARef<File>` pointers.
///
/// ## Light refcounts
///
/// Whenever a process has an fd to a file, it may use something called a "light refcount" as a
/// performance optimization. Light refcounts are acquired by calling `fdget` and released with
/// `fdput`. The idea behind light refcounts is that if the fd is not closed between the calls to
/// `fdget` and `fdput`, then the refcount cannot hit zero during that time, as the `struct
/// files_struct` holds a reference until the fd is closed. This means that it's safe to access the
/// file even if `fdget` does not increment the refcount.
///
/// The requirement that the fd is not closed during a light refcount applies globally across all
/// threads - not just on the thread using the light refcount. For this reason, light refcounts are
/// only used when the `struct files_struct` is not shared with other threads, since this ensures
/// that other unrelated threads cannot suddenly start using the fd and close it. Therefore,
/// calling `fdget` on a shared `struct files_struct` creates a normal refcount instead of a light
/// refcount.
///
/// Light reference counts must be released with `fdput` before the system call returns to
/// userspace. This means that if you wait until the current system call returns to userspace, then
/// all light refcounts that existed at the time have gone away.
///
/// ### The file position
///
/// Each `struct file` has a position integer, which is protected by the `f_pos_lock` mutex.
/// However, if the `struct file` is not shared, then the kernel may avoid taking the lock as a
/// performance optimization.
///
/// The condition for avoiding the `f_pos_lock` mutex is different from the condition for using
/// `fdget`. With `fdget`, you may avoid incrementing the refcount as long as the current fd table
/// is not shared; it is okay if there are other fd tables that also reference the same `struct
/// file`. However, `fdget_pos` can only avoid taking the `f_pos_lock` if the entire `struct file`
/// is not shared, as different processes with an fd to the same `struct file` share the same
/// position.
///
/// To represent files that are not thread safe due to this optimization, the [`LocalFile`] type is
/// used.
///
/// ## Rust references
///
/// The reference type `&File` is similar to light refcounts:
///
/// * `&File` references don't own a reference count. They can only exist as long as the reference
///   count stays positive, and can only be created when there is some mechanism in place to ensure
///   this.
///
/// * The Rust borrow-checker normally ensures this by enforcing that the `ARef<File>` from which
///   a `&File` is created outlives the `&File`.
///
/// * Using the unsafe [`File::from_raw_file`] means that it is up to the caller to ensure that the
///   `&File` only exists while the reference count is positive.
///
/// * You can think of `fdget` as using an fd to look up an `ARef<File>` in the `struct
///   files_struct` and create an `&File` from it. The "fd cannot be closed" rule is like the Rust
///   rule "the `ARef<File>` must outlive the `&File`".
///
/// # Invariants
///
/// * All instances of this type are refcounted using the `f_count` field.
/// * There must not be any active calls to `fdget_pos` on this file that did not take the
///   `f_pos_lock` mutex.
#[repr(transparent)]
pub struct File<T: FileSystem + ?Sized = UnspecifiedFS> {
    inner: Opaque<bindings::file>,
    _p: PhantomData<T>,
}

// SAFETY: This file is known to not have any active `fdget_pos` calls that did not take the
// `f_pos_lock` mutex, so it is safe to transfer it between threads.
unsafe impl<T: FileSystem + ?Sized> Send for File<T> {}

// SAFETY: This file is known to not have any active `fdget_pos` calls that did not take the
// `f_pos_lock` mutex, so it is safe to access its methods from several threads in parallel.
unsafe impl<T: FileSystem + ?Sized> Sync for File<T> {}

// SAFETY: The type invariants guarantee that `File` is always ref-counted. This implementation
// makes `ARef<File>` own a normal refcount.
unsafe impl<T: FileSystem + ?Sized> AlwaysRefCounted for File<T> {
    #[inline]
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_file(self.as_ptr()) };
    }

    #[inline]
    unsafe fn dec_ref(obj: ptr::NonNull<File<T>>) {
        // SAFETY: To call this method, the caller passes us ownership of a normal refcount, so we
        // may drop it. The cast is okay since `File` has the same representation as `struct file`.
        unsafe { bindings::fput(obj.cast().as_ptr()) }
    }
}

/// Wraps the kernel's `struct file`. Not thread safe.
///
/// This type represents a file that is not known to be safe to transfer across thread boundaries.
/// To obtain a thread-safe [`File`], use the [`assume_no_fdget_pos`] conversion.
///
/// See the documentation for [`File`] for more information.
///
/// # Invariants
///
/// * All instances of this type are refcounted using the `f_count` field.
/// * If there is an active call to `fdget_pos` that did not take the `f_pos_lock` mutex, then it
///   must be on the same thread as this file.
///
/// [`assume_no_fdget_pos`]: LocalFile::assume_no_fdget_pos
#[repr(transparent)]
pub struct LocalFile<T: FileSystem + ?Sized = UnspecifiedFS> {
    inner: Opaque<bindings::file>,
    _p: PhantomData<T>,
}

// SAFETY: The type invariants guarantee that `LocalFile` is always ref-counted. This implementation
// makes `ARef<LocalFile>` own a normal refcount.
unsafe impl<T: FileSystem + ?Sized> AlwaysRefCounted for LocalFile<T> {
    #[inline]
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_file(self.as_ptr()) };
    }

    #[inline]
    unsafe fn dec_ref(obj: ptr::NonNull<LocalFile<T>>) {
        // SAFETY: To call this method, the caller passes us ownership of a normal refcount, so we
        // may drop it. The cast is okay since `LocalFile` has the same representation as
        // `struct file`.
        unsafe { bindings::fput(obj.cast().as_ptr()) }
    }
}

impl<T: FileSystem + ?Sized> LocalFile<T> {
    /// Constructs a new `struct file` wrapper from a file descriptor.
    ///
    /// The file descriptor belongs to the current process, and there might be active local calls
    /// to `fdget_pos` on the same file.
    ///
    /// To obtain an `ARef<File>`, use the [`assume_no_fdget_pos`] function to convert.
    ///
    /// [`assume_no_fdget_pos`]: LocalFile::assume_no_fdget_pos
    #[inline]
    pub fn fget(fd: u32) -> Result<ARef<LocalFile<T>>, BadFdError> {
        // SAFETY: FFI call, there are no requirements on `fd`.
        let ptr = ptr::NonNull::new(unsafe { bindings::fget(fd) }).ok_or(BadFdError)?;

        // SAFETY: `bindings::fget` created a refcount, and we pass ownership of it to the `ARef`.
        //
        // INVARIANT: This file is in the fd table on this thread, so either all `fdget_pos` calls
        // are on this thread, or the file is shared, in which case `fdget_pos` calls took the
        // `f_pos_lock` mutex.
        Ok(unsafe { ARef::from_raw(ptr.cast()) })
    }

    /// Creates a reference to a [`LocalFile`] from a valid pointer.
    ///
    /// # Safety
    ///
    /// * The caller must ensure that `ptr` points at a valid file and that the file's refcount is
    ///   positive for the duration of `'a`.
    /// * The caller must ensure that if there is an active call to `fdget_pos` that did not take
    ///   the `f_pos_lock` mutex, then that call is on the current thread.
    #[inline]
    pub unsafe fn from_raw_file<'a>(ptr: *const bindings::file) -> &'a LocalFile<T> {
        // SAFETY: The caller guarantees that the pointer is not dangling and stays valid for the
        // duration of `'a`. The cast is okay because `LocalFile` is `repr(transparent)`.
        //
        // INVARIANT: The caller guarantees that there are no problematic `fdget_pos` calls.
        unsafe { &*ptr.cast() }
    }

    /// Assume that there are no active `fdget_pos` calls that prevent us from sharing this file.
    ///
    /// This makes it safe to transfer this file to other threads. No checks are performed, and
    /// using it incorrectly may lead to a data race on the file position if the file is shared
    /// with another thread.
    ///
    /// This method is intended to be used together with [`LocalFile::fget`] when the caller knows
    /// statically that there are no `fdget_pos` calls on the current thread. For example, you
    /// might use it when calling `fget` from an ioctl, since ioctls usually do not touch the file
    /// position.
    ///
    /// # Safety
    ///
    /// There must not be any active `fdget_pos` calls on the current thread.
    #[inline]
    pub unsafe fn assume_no_fdget_pos(me: ARef<LocalFile<T>>) -> ARef<File<T>> {
        // INVARIANT: There are no `fdget_pos` calls on the current thread, and by the type
        // invariants, if there is a `fdget_pos` call on another thread, then it took the
        // `f_pos_lock` mutex.
        //
        // SAFETY: `LocalFile` and `File` have the same layout.
        unsafe { ARef::from_raw(ARef::into_raw(me).cast()) }
    }

    /// Returns a raw pointer to the inner C struct.
    #[inline]
    pub fn as_ptr(&self) -> *mut bindings::file {
        self.inner.get()
    }

    /// Returns the credentials of the task that originally opened the file.
    pub fn cred(&self) -> &Credential {
        // SAFETY: It's okay to read the `f_cred` field without synchronization because `f_cred` is
        // never changed after initialization of the file.
        let ptr = unsafe { (*self.as_ptr()).f_cred };

        // SAFETY: The signature of this function ensures that the caller will only access the
        // returned credential while the file is still valid, and the C side ensures that the
        // credential stays valid at least as long as the file.
        unsafe { Credential::from_ptr(ptr) }
    }

    /// Returns the flags associated with the file.
    ///
    /// The flags are a combination of the constants in [`flags`].
    #[inline]
    pub fn flags(&self) -> u32 {
        // This `read_volatile` is intended to correspond to a READ_ONCE call.
        //
        // SAFETY: The file is valid because the shared reference guarantees a nonzero refcount.
        //
        // FIXME(read_once): Replace with `read_once` when available on the Rust side.
        unsafe { core::ptr::addr_of!((*self.as_ptr()).f_flags).read_volatile() }
    }

    /// Returns the inode associated with the file.
    pub fn inode(&self) -> &INode<T> {
        // SAFETY: `f_inode` is an immutable field, so it's safe to read it.
        unsafe { INode::from_raw((*self.inner.get()).f_inode) }
    }

    /// Returns the host inode of that file
    pub fn host_inode(&self) -> &INode<T> {
        // SAFETY: f_mapping is always valid and requires a host
        unsafe { INode::from_raw((*(*self.inner.get()).f_mapping).host) }
    }

    /// Returns the dentry associated with the file.
    pub fn dentry(&self) -> &DEntry<T> {
        // SAFETY: `f_path` is an immutable field, so it's safe to read it. And will remain safe to
        // read while the `&self` is valid.
        unsafe { DEntry::from_raw((*self.inner.get()).__bindgen_anon_1.f_path.dentry) }
    }

    pub fn parent_ino(&self) -> usize {
        let dentry = self.dentry().0.get();

        // SAFETY: dentry is guarenteed to be valid, thus okay to get its parent_ino
        unsafe { bindings::d_parent_ino(dentry) }
    }
}

impl<T: FileSystem + ?Sized> File<T> {
    /// Creates a reference to a [`File`] from a valid pointer.
    ///
    /// # Safety
    ///
    /// * The caller must ensure that `ptr` points at a valid file and that the file's refcount is
    ///   positive for the duration of `'a`.
    /// * The caller must ensure that if there are active `fdget_pos` calls on this file, then they
    ///   took the `f_pos_lock` mutex.
    #[inline]
    pub unsafe fn from_raw_file<'a>(ptr: *const bindings::file) -> &'a File<T> {
        // SAFETY: The caller guarantees that the pointer is not dangling and stays valid for the
        // duration of `'a`. The cast is okay because `File` is `repr(transparent)`.
        //
        // INVARIANT: The caller guarantees that there are no problematic `fdget_pos` calls.
        unsafe { &*ptr.cast() }
    }
}

// Make LocalFile methods available on File.
impl<T: FileSystem + ?Sized> core::ops::Deref for File<T> {
    type Target = LocalFile<T>;
    #[inline]
    fn deref(&self) -> &LocalFile<T> {
        // SAFETY: The caller provides a `&File`, and since it is a reference, it must point at a
        // valid file for the desired duration.
        //
        // By the type invariants, there are no `fdget_pos` calls that did not take the
        // `f_pos_lock` mutex.
        unsafe { LocalFile::from_raw_file(core::ptr::from_ref(self).cast()) }
    }
}

/// A file descriptor reservation.
///
/// This allows the creation of a file descriptor in two steps: first, we reserve a slot for it,
/// then we commit or drop the reservation. The first step may fail (e.g., the current process ran
/// out of available slots), but commit and drop never fail (and are mutually exclusive).
///
/// Dropping the reservation happens in the destructor of this type.
///
/// # Invariants
///
/// The fd stored in this struct must correspond to a reserved file descriptor of the current task.
pub struct FileDescriptorReservation {
    fd: u32,
    /// Prevent values of this type from being moved to a different task.
    ///
    /// The `fd_install` and `put_unused_fd` functions assume that the value of `current` is
    /// unchanged since the call to `get_unused_fd_flags`. By adding this marker to this type, we
    /// prevent it from being moved across task boundaries, which ensures that `current` does not
    /// change while this value exists.
    _not_send: NotThreadSafe,
}

impl FileDescriptorReservation {
    /// Creates a new file descriptor reservation.
    #[inline]
    pub fn get_unused_fd_flags(flags: u32) -> Result<Self> {
        // SAFETY: FFI call, there are no safety requirements on `flags`.
        let fd: i32 = unsafe { bindings::get_unused_fd_flags(flags) };
        to_result(fd)?;

        Ok(Self {
            fd: fd as u32,
            _not_send: NotThreadSafe,
        })
    }

    /// Returns the file descriptor number that was reserved.
    #[inline]
    pub fn reserved_fd(&self) -> u32 {
        self.fd
    }

    /// Commits the reservation.
    ///
    /// The previously reserved file descriptor is bound to `file`. This method consumes the
    /// [`FileDescriptorReservation`], so it will not be usable after this call.
    #[inline]
    pub fn fd_install(self, file: ARef<File>) {
        // SAFETY: `self.fd` was previously returned by `get_unused_fd_flags`. We have not yet used
        // the fd, so it is still valid, and `current` still refers to the same task, as this type
        // cannot be moved across task boundaries.
        //
        // Furthermore, the file pointer is guaranteed to own a refcount by its type invariants,
        // and we take ownership of that refcount by not running the destructor below.
        // Additionally, the file is known to not have any non-shared `fdget_pos` calls, so even if
        // this process starts using the file position, this will not result in a data race on the
        // file position.
        unsafe { bindings::fd_install(self.fd, file.as_ptr()) };

        // `fd_install` consumes both the file descriptor and the file reference, so we cannot run
        // the destructors.
        core::mem::forget(self);
        core::mem::forget(file);
    }
}

impl Drop for FileDescriptorReservation {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: By the type invariants of this type, `self.fd` was previously returned by
        // `get_unused_fd_flags`. We have not yet used the fd, so it is still valid, and `current`
        // still refers to the same task, as this type cannot be moved across task boundaries.
        unsafe { bindings::put_unused_fd(self.fd) };
    }
}

/// Represents the [`EBADF`] error code.
///
/// Used for methods that can only fail with [`EBADF`].
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct BadFdError;

impl From<BadFdError> for Error {
    #[inline]
    fn from(_: BadFdError) -> Error {
        EBADF
    }
}

impl fmt::Debug for BadFdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("EBADF")
    }
}

/// Indicates how to interpret the `offset` argument in [`Operations::seek`].
#[repr(u32)]
pub enum Whence {
    /// `offset` bytes from the start of the file.
    Set = bindings::SEEK_SET,

    /// `offset` bytes from the end of the file.
    End = bindings::SEEK_END,

    /// `offset` bytes from the current location.
    Cur = bindings::SEEK_CUR,

    /// The next location greater than or equal to `offset` that contains data.
    Data = bindings::SEEK_DATA,

    /// The next location greater than or equal to `offset` that contains a hole.
    Hole = bindings::SEEK_HOLE,
}

impl TryFrom<i32> for Whence {
    type Error = crate::error::Error;

    fn try_from(v: i32) -> Result<Self> {
        match v {
            v if v == Self::Set as i32 => Ok(Self::Set),
            v if v == Self::End as i32 => Ok(Self::End),
            v if v == Self::Cur as i32 => Ok(Self::Cur),
            v if v == Self::Data as i32 => Ok(Self::Data),
            v if v == Self::Hole as i32 => Ok(Self::Hole),
            _ => Err(EDOM),
        }
    }
}

/// Generic implementation of [`Operations::seek`].
pub fn generic_seek(
    file: &File<impl FileSystem + ?Sized>,
    offset: Offset,
    whence: Whence,
) -> Result<Offset> {
    // SAFETY: generic_file_llseek returns an offset (err if n < 0) which is dealt with below
    let n = unsafe { bindings::generic_file_llseek(file.inner.get(), offset, whence as i32) };
    if n < 0 {
        Err(Error::from_errno(n.try_into()?))
    } else {
        Ok(n)
    }
}

/// Operations implemented by files
#[vtable]
pub trait Operations {
    /// File system that these operations are compatible with.
    type FileSystem: FileSystem + ?Sized;

    /// Reads data from this file into caller's buffer.
    fn read(
        _file: &File<Self::FileSystem>,
        _buffer: &mut user::Writer,
        _offset: &mut Offset,
    ) -> Result<usize> {
        Err(EINVAL)
    }

    fn read_iter(
        _kiocb: Kiocb<'_, <Self::FileSystem as FileSystem>::Data>,
        _iov: &mut IovIterDest<'_>,
    ) -> Result<usize> {
        Err(EINVAL)
    }

    /// Seeks the file to the given offset.
    fn seek(_file: &File<Self::FileSystem>, _offset: Offset, _whence: Whence) -> Result<Offset> {
        Err(EINVAL)
    }

    /// Reads directory entries from directory files.
    ///
    /// [`DirEmitter::pos`] holds the current position of the directory reader.
    fn read_dir(
        _file: &File<Self::FileSystem>,
        _inode: &Locked<&INode<Self::FileSystem>, inode::ReadSem>,
        _emitter: &mut DirEmitter,
    ) -> Result {
        Err(EINVAL)
    }

    /// Write to this file
    fn write_iter(
        _kiocb: Kiocb<'_, <Self::FileSystem as FileSystem>::Data>,
        _iov: &mut IovIterSource<'_>,
    ) -> Result<usize> {
        Err(EINVAL)
    }
}

/// Represents file operations
pub struct Ops<T: FileSystem + ?Sized> {
    pub(crate) inner: *const bindings::file_operations,
    _p: PhantomData<T>,
}

impl<T: FileSystem + ?Sized> Ops<T> {
    /// Returns file operations for page-cache-based ro files.
    pub fn generic_ro_file() -> Self {
        Self {
            // SAFETY: This is a constant in C, it never changes.
            inner: unsafe { &bindings::generic_ro_fops },
            _p: PhantomData,
        }
    }
    /// Creates file operations from a type that implements the [`Operations`] trait.
    pub const fn new_file<U: Operations<FileSystem = T> + ?Sized>() -> Self {
        struct Table<T: Operations + ?Sized>(PhantomData<T>);
        impl<T: Operations + ?Sized> Table<T> {
            const TABLE: bindings::file_operations = bindings::file_operations {
                owner: ptr::null_mut(),
                llseek: if T::HAS_SEEK {
                    Some(Self::seek_callback)
                } else {
                    None
                },
                read: if T::HAS_READ {
                    Some(Self::read_callback)
                } else {
                    None
                },
                write: None,
                read_iter: Some(Self::read_iter_callback),
                write_iter: if T::HAS_WRITE_ITER {
                    Some(Self::write_iter_callback)
                } else {
                    None
                },
                iopoll: None,
                iterate_shared: None,
                poll: None,
                unlocked_ioctl: None,
                fop_flags: 0,
                compat_ioctl: None,
                mmap: Some(bindings::generic_file_mmap),
                mmap_prepare: None,
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
                splice_read: Some(bindings::filemap_splice_read),
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

            /// Seek into a file, file system dependant
            ///
            /// # Safety
            ///
            /// `file_ptr` must be a reference to a valid `struct file`
            unsafe extern "C" fn seek_callback(
                file_ptr: *mut bindings::file,
                offset: bindings::loff_t,
                whence: i32,
            ) -> bindings::loff_t {
                from_result(|| {
                    // SAFETY: The C API guarantees that `file` is valid for the duration of the
                    // callback. Since this callback is specifically for filesystem T, we know `T`
                    // is the right filesystem.
                    let file = unsafe { File::from_raw_file(file_ptr) };
                    T::seek(file, offset, whence.try_into()?)
                })
            }

            /// Read a file, file system dependant
            ///
            /// # Safety
            ///
            /// `file_ptr` must be a reference to a valid `struct file`
            /// `ptr` must point to allocated memory which is at least of `len` bytes
            unsafe extern "C" fn read_callback(
                file_ptr: *mut bindings::file,
                ptr: *mut core::ffi::c_char,
                len: usize,
                offset: *mut bindings::loff_t,
            ) -> isize {
                from_result(|| {
                    // SAFETY: The C API guarantees that `file` is valid for the duration of the
                    // callback. Since this callback is specifically for filesystem T, we know `T`
                    // is the right filesystem.
                    let file = unsafe { File::from_raw_file(file_ptr) };
                    let mut writer = user::Writer::new(ptr, len);

                    // SAFETY: The C API guarantees that `offset` is valid for read and write.
                    let read = T::read(file, &mut writer, unsafe { &mut *offset })?;
                    Ok(isize::try_from(read)?)
                })
            }

            // /// # Safety
            // ///
            // /// `kiocb` must be correspond to a valid file that is associated with a
            // /// `T`. `iter` must be a valid `struct iov_iter` for writing.
            // unsafe extern "C" fn read_iter_callback(
            //     kiocb: *mut bindings::kiocb,
            //     iter: *mut bindings::iov_iter,
            // ) -> isize {
            //     // SAFETY: The caller provides a valid `struct kiocb` associated with a
            //     // `MiscDeviceRegistration<T>` file.
            //     let kiocb = unsafe { Kiocb::from_raw(kiocb) };
            //     // SAFETY: This is a valid `struct iov_iter` for writing.
            //     let iov = unsafe { IovIterDest::from_raw(iter) };
            //
            //     match T::read_iter(kiocb, iov) {
            //         Ok(res) => res as isize,
            //         Err(err) => err.to_errno() as isize,
            //     }
            // }

            /// # Safety
            ///
            /// `kiocb` must be correspond to a valid file that is associated with a
            /// `T`. `iter` must be a valid `struct iov_iter` for writing.
            unsafe extern "C" fn read_iter_callback(
                kiocb: *mut bindings::kiocb,
                iter: *mut bindings::iov_iter,
            ) -> isize {
                // SAFETY: returns copied bytes, caller deals with error (negative numbers, hence
                // isize), should be improved to be specifible instead of relying on generic
                unsafe { bindings::generic_file_read_iter(kiocb, iter) }
            }

            /// # Safety
            ///
            /// `kiocb` must be correspond to a valid file that is associated with a
            /// `<T>`. `iter` must be a valid `struct iov_iter` for writing.
            unsafe extern "C" fn write_iter_callback(
                kiocb: *mut bindings::kiocb,
                iter: *mut bindings::iov_iter,
            ) -> isize {
                from_result(|| {
                    // SAFETY: The caller provides a valid `struct kiocb` associated with a
                    // `<T>` file.
                    let kiocb = unsafe { Kiocb::from_raw(kiocb) };
                    // SAFETY: This is a valid `struct iov_iter` for reading.
                    let iov = unsafe { IovIterSource::from_raw(iter) };

                    let wrote = T::write_iter(kiocb, iov)?;

                    Ok(wrote.try_into()?)
                })
            }
        }
        Self {
            inner: &Table::<U>::TABLE,
            _p: PhantomData,
        }
    }

    /// Creates file operations from a type that implements the [`Operations`] trait.
    pub const fn new_dir<U: Operations<FileSystem = T> + ?Sized>() -> Self {
        struct Table<T: Operations + ?Sized>(PhantomData<T>);
        impl<T: Operations + ?Sized> Table<T> {
            const TABLE: bindings::file_operations = bindings::file_operations {
                owner: ptr::null_mut(),
                llseek: None,
                read: None,
                write: None,
                read_iter: None,
                write_iter: None,
                iopoll: None,
                iterate_shared: if T::HAS_READ_DIR {
                    Some(Self::read_dir_callback)
                } else {
                    None
                },
                poll: None,
                unlocked_ioctl: None,
                fop_flags: 0,
                compat_ioctl: None,
                mmap: None,
                mmap_prepare: None,
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

            /// Read a directory, file system dependant
            ///
            /// # Safety
            ///
            /// `file_ptr` must be a reference to a valid `struct file`
            /// `ctx_ptr` must be a reference to a valid `struct dir_context`
            unsafe extern "C" fn read_dir_callback(
                file_ptr: *mut bindings::file,
                ctx_ptr: *mut bindings::dir_context,
            ) -> core::ffi::c_int {
                from_result(|| {
                    // SAFETY: The C API guarantees that `file` is valid for the duration of the
                    // callback. Since this callback is specifically for filesystem T, we know `T`
                    // is the right filesystem.
                    let file = unsafe { File::from_raw_file(file_ptr) };

                    // SAFETY: The C API guarantees that this is the only reference to the
                    // `dir_context` instance.
                    let emitter = unsafe { &mut *ctx_ptr.cast::<DirEmitter>() };
                    let orig_pos = emitter.pos();

                    // SAFETY: The C API guarantees that the inode's rw semaphore is locked in read
                    // mode. It does not expect callees to unlock it, so we make the locked object
                    // manually dropped to avoid unlocking it.
                    let locked = ManuallyDrop::new(unsafe { Locked::new(file.inode()) });

                    // Call the module implementation. We ignore errors if directory entries have
                    // been succesfully emitted: this is because we want users to see them before
                    // the error.
                    match T::read_dir(file, &locked, emitter) {
                        Ok(()) => Ok(0),
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
        }
        Self {
            inner: &Table::<U>::TABLE,
            _p: PhantomData,
        }
    }
}

/// The types of directory entries reported by [`Operations::read_dir`].
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

impl DirEntryType {
    pub fn from_mode(mode: u32) -> Self {
        let dt_value = (mode & bindings::S_IFMT) >> bindings::S_DT_SHIFT;
        Self::try_from(dt_value).unwrap_or(Self::Unknown)
    }
}

impl From<&inode::Type> for DirEntryType {
    fn from(value: &inode::Type) -> Self {
        match value {
            inode::Type::Fifo => DirEntryType::Fifo,
            inode::Type::Chr(_, _) => DirEntryType::Chr,
            inode::Type::Dir => DirEntryType::Dir,
            inode::Type::Blk(_, _) => DirEntryType::Blk,
            inode::Type::Reg => DirEntryType::Reg,
            inode::Type::Lnk(_) => DirEntryType::Lnk,
            inode::Type::Sock => DirEntryType::Sock,
        }
    }
}

impl TryFrom<u32> for DirEntryType {
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

/// Directory entry emitter.
///
/// This is used in [`Operations::read_dir`] implementations to report the directory entry.
#[repr(transparent)]
pub struct DirEmitter(bindings::dir_context);

impl DirEmitter {
    /// Returns the current position of the emitter.
    pub fn pos(&self) -> Offset {
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
    pub fn emit(&mut self, pos_inc: Offset, name: &[u8], ino: u64, etype: DirEntryType) -> bool {
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
                name.as_ptr(),
                name_len,
                self.0.pos,
                ino,
                etype as u32,
            )
        };
        if ret {
            self.0.pos = new_pos;
        }
        ret
    }

    pub fn emit_dots<T: FileSystem + ?Sized>(&mut self, file: &File<T>) -> bool {
        if self.0.pos == 0 && !self.emit(1, b".", file.inode().ino() as u64, DirEntryType::Dir) {
            return false;
        }

        if self.0.pos == 1 && !self.emit(1, b"..", file.parent_ino() as u64, DirEntryType::Dir) {
            return false;
        }

        true
    }
}
