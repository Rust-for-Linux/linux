// SPDX-License-Identifier: GPL-2.0

//! File operations.
//!
//! C header: [`include/linux/fs.h`](../../../../include/linux/fs.h)

use core::convert::{TryFrom, TryInto};
use core::{
    marker, mem,
    ptr::{self, NonNull},
};

use alloc::boxed::Box;

use crate::{
    bindings, c_types,
    error::{Error, Result},
    file::File,
    from_kernel_result,
    io_buffer::{IoBufferReader, IoBufferWriter},
    iov_iter::IovIter,
    sync::CondVar,
    types::PointerWrapper,
    user_ptr::{UserSlicePtr, UserSlicePtrReader, UserSlicePtrWriter},
};

/// Wraps the kernel's `struct poll_table_struct`.
///
/// # Invariants
///
/// The pointer [`PollTable::ptr`] is valid.
pub struct PollTable {
    ptr: Option<NonNull<bindings::poll_table_struct>>,
}

impl PollTable {
    /// Constructors a new `struct poll_table_struct` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be either null or a valid pointer for the lifetime of the object.
    unsafe fn from_ptr(ptr: *mut bindings::poll_table_struct) -> Self {
        Self {
            ptr: NonNull::new(ptr),
        }
    }

    /// Associates the given file and condition variable to this poll table. It means notifying the
    /// condition variable will notify the poll table as well; additionally, the association
    /// between the condition variable and the file will automatically be undone by the kernel when
    /// the file is destructed. To unilaterally remove the association before then, one can call
    /// [`CondVar::free_waiters`].
    ///
    /// # Safety
    ///
    /// If the condition variable is destroyed before the file, then [`CondVar::free_waiters`] must
    /// be called to ensure that all waiters are flushed out.
    pub unsafe fn register_wait<'a>(&self, file: &'a File, cv: &'a CondVar) {
        if let Some(ptr) = self.ptr {
            // SAFETY: `PollTable::ptr` is guaranteed to be valid by the type invariants.
            let table = ptr.as_ref();
            if let Some(proc) = table._qproc {
                // SAFETY: All pointers are known to be valid.
                proc(file.ptr.as_ptr(), cv.wait_list.get(), ptr.as_ptr())
            }
        }
    }
}

/// Equivalent to [`std::io::SeekFrom`].
///
/// [`std::io::SeekFrom`]: https://doc.rust-lang.org/std/io/enum.SeekFrom.html
pub enum SeekFrom {
    /// Equivalent to C's `SEEK_SET`.
    Start(u64),

    /// Equivalent to C's `SEEK_END`.
    End(i64),

    /// Equivalent to C's `SEEK_CUR`.
    Current(i64),
}

unsafe extern "C" fn open_callback<A: FileOpenAdapter, T: FileOpener<A::Arg>>(
    inode: *mut bindings::inode,
    file: *mut bindings::file,
) -> c_types::c_int {
    from_kernel_result! {
        let mut file = NonNull::new_unchecked(file);

        let arg = A::convert(inode, file.as_ptr());
        let ptr = T::open(&*arg)?.into_pointer();
        file.as_mut().private_data = ptr as *mut c_types::c_void;
        Ok(0)
    }
}

/// Creates a shared reference to a file's `private_data` field interpreted as `T`
///
/// # SAFETY
/// - `file` must point to a valid [`bindings::file`]
/// - the [`bindings::file`]'s `private_data` field must point to a valid `T` that lives for `'a`
unsafe fn get_file_private_data<'a, T>(file: NonNull<bindings::file>) -> &'a T {
    &*file.as_ref().private_data.cast::<T>()
}

unsafe extern "C" fn read_callback<T: FileOperations>(
    file: *mut bindings::file,
    buf: *mut c_types::c_char,
    len: c_types::c_size_t,
    offset: *mut bindings::loff_t,
) -> c_types::c_ssize_t {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);
        let offset = &mut *offset;

        let mut data = UserSlicePtr::new(buf as *mut c_types::c_void, len).writer();
        let f: &T = get_file_private_data(file);
        // No `FMODE_UNSIGNED_OFFSET` support, so `offset` must be in [0, 2^63).
        // See discussion in https://github.com/fishinabarrel/linux-kernel-module-rust/pull/113
        let read = f.read(&File::from_ptr(file), &mut data, (*offset).try_into()?)?;
        *offset += bindings::loff_t::try_from(read).unwrap();
        Ok(read as _)
    }
}

unsafe extern "C" fn read_iter_callback<T: FileOperations>(
    iocb: *mut bindings::kiocb,
    raw_iter: *mut bindings::iov_iter,
) -> isize {
    from_kernel_result! {
        let raw_iter = NonNull::new_unchecked(raw_iter);
        let iocb = &mut *iocb;
        let file = NonNull::new_unchecked(iocb.ki_filp);

        let mut iter = IovIter::from_ptr(raw_iter);
        let offset = iocb.ki_pos;
        let f: &T = get_file_private_data(file);
        let read = f.read(&File::from_ptr(file), &mut iter, offset.try_into()?)?;
        iocb.ki_pos += bindings::loff_t::try_from(read).unwrap();
        Ok(read as _)
    }
}

unsafe extern "C" fn write_callback<T: FileOperations>(
    file: *mut bindings::file,
    buf: *const c_types::c_char,
    len: c_types::c_size_t,
    offset: *mut bindings::loff_t,
) -> c_types::c_ssize_t {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);
        let offset = &mut *offset;

        // TODO: why is it okay to upgrade the mutability of `buf` here?
        let mut data = UserSlicePtr::new(buf as *mut c_types::c_void, len).reader();
        let f: &T = get_file_private_data(file);
        // No `FMODE_UNSIGNED_OFFSET` support, so `offset` must be in [0, 2^63).
        // See discussion in https://github.com/fishinabarrel/linux-kernel-module-rust/pull/113
        let written = f.write(&File::from_ptr(file), &mut data, (*offset).try_into()?)?;
        *offset += bindings::loff_t::try_from(written).unwrap();
        Ok(written as _)
    }
}

unsafe extern "C" fn write_iter_callback<T: FileOperations>(
    iocb: *mut bindings::kiocb,
    raw_iter: *mut bindings::iov_iter,
) -> isize {
    from_kernel_result! {
        let raw_iter = NonNull::new_unchecked(raw_iter);
        let iocb = &mut *iocb;

        let file = NonNull::new_unchecked(iocb.ki_filp);
        let mut iter = IovIter::from_ptr(raw_iter);

        let offset = iocb.ki_pos;
        let f: &T = get_file_private_data(file);
        let written = f.write(&File::from_ptr(file), &mut iter, offset.try_into()?)?;
        iocb.ki_pos += bindings::loff_t::try_from(written).unwrap();
        Ok(written as _)
    }
}

unsafe extern "C" fn release_callback<T: FileOperations>(
    _inode: *mut bindings::inode,
    file: *mut bindings::file,
) -> c_types::c_int {
    let mut file = NonNull::new_unchecked(file);

    let ptr = mem::replace(&mut file.as_mut().private_data, ptr::null_mut());
    T::release(T::Wrapper::from_pointer(ptr as _), &File::from_ptr(file));
    0
}

unsafe extern "C" fn llseek_callback<T: FileOperations>(
    file: *mut bindings::file,
    offset: bindings::loff_t,
    whence: c_types::c_int,
) -> bindings::loff_t {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);

        let off = match whence as u32 {
            bindings::SEEK_SET => SeekFrom::Start(offset.try_into()?),
            bindings::SEEK_CUR => SeekFrom::Current(offset),
            bindings::SEEK_END => SeekFrom::End(offset),
            _ => return Err(Error::EINVAL),
        };
        let f: &T = get_file_private_data(file);
        let off = f.seek(&File::from_ptr(file), off)?;
        Ok(off as bindings::loff_t)
    }
}

unsafe extern "C" fn unlocked_ioctl_callback<T: FileOperations>(
    file: *mut bindings::file,
    cmd: c_types::c_uint,
    arg: c_types::c_ulong,
) -> c_types::c_long {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);

        let f: &T = get_file_private_data(file);
        // SAFETY: This function is called by the kernel, so it must set `fs` appropriately.
        let mut cmd = IoctlCommand::new(cmd as _, arg as _);
        let ret = f.ioctl(&File::from_ptr(file), &mut cmd)?;
        Ok(ret as _)
    }
}

unsafe extern "C" fn compat_ioctl_callback<T: FileOperations>(
    file: *mut bindings::file,
    cmd: c_types::c_uint,
    arg: c_types::c_ulong,
) -> c_types::c_long {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);

        let f: &T = get_file_private_data(file);
        // SAFETY: This function is called by the kernel, so it must set `fs` appropriately.
        let mut cmd = IoctlCommand::new(cmd as _, arg as _);
        let ret = f.compat_ioctl(&File::from_ptr(file), &mut cmd)?;
        Ok(ret as _)
    }
}

unsafe extern "C" fn mmap_callback<T: FileOperations>(
    file: *mut bindings::file,
    vma: *mut bindings::vm_area_struct,
) -> c_types::c_int {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);
        let mut vma = NonNull::new_unchecked(vma);

        let f: &T = get_file_private_data(file);
        f.mmap(&File::from_ptr(file), vma.as_mut())?;
        Ok(0)
    }
}

unsafe extern "C" fn fsync_callback<T: FileOperations>(
    file: *mut bindings::file,
    start: bindings::loff_t,
    end: bindings::loff_t,
    datasync: c_types::c_int,
) -> c_types::c_int {
    from_kernel_result! {
        let file = NonNull::new_unchecked(file);

        let start = start.try_into()?;
        let end = end.try_into()?;
        let datasync = datasync != 0;

        let f: &T = get_file_private_data(file);
        let res = f.fsync(&File::from_ptr(file), start, end, datasync)?;
        Ok(res.try_into().unwrap())
    }
}

unsafe extern "C" fn poll_callback<T: FileOperations>(
    file: *mut bindings::file,
    wait: *mut bindings::poll_table_struct,
) -> bindings::__poll_t {
    let file = NonNull::new_unchecked(file);

    let f: &T = get_file_private_data(file);
    f.poll(&File::from_ptr(file), &PollTable::from_ptr(wait))
        .unwrap_or(bindings::POLLERR)
}

pub(crate) struct FileOperationsVtable<A, T>(marker::PhantomData<A>, marker::PhantomData<T>);

impl<A: FileOpenAdapter, T: FileOpener<A::Arg>> FileOperationsVtable<A, T> {
    const VTABLE: bindings::file_operations = bindings::file_operations {
        open: Some(open_callback::<A, T>),
        release: Some(release_callback::<T>),
        read: if T::TO_USE.read {
            Some(read_callback::<T>)
        } else {
            None
        },
        write: if T::TO_USE.write {
            Some(write_callback::<T>)
        } else {
            None
        },
        llseek: if T::TO_USE.seek {
            Some(llseek_callback::<T>)
        } else {
            None
        },

        check_flags: None,
        compat_ioctl: if T::TO_USE.compat_ioctl {
            Some(compat_ioctl_callback::<T>)
        } else {
            None
        },
        copy_file_range: None,
        fallocate: None,
        fadvise: None,
        fasync: None,
        flock: None,
        flush: None,
        fsync: if T::TO_USE.fsync {
            Some(fsync_callback::<T>)
        } else {
            None
        },
        get_unmapped_area: None,
        iterate: None,
        iterate_shared: None,
        iopoll: None,
        lock: None,
        mmap: if T::TO_USE.mmap {
            Some(mmap_callback::<T>)
        } else {
            None
        },
        mmap_supported_flags: 0,
        owner: ptr::null_mut(),
        poll: if T::TO_USE.poll {
            Some(poll_callback::<T>)
        } else {
            None
        },
        read_iter: if T::TO_USE.read_iter {
            Some(read_iter_callback::<T>)
        } else {
            None
        },
        remap_file_range: None,
        sendpage: None,
        setlease: None,
        show_fdinfo: None,
        splice_read: None,
        splice_write: None,
        unlocked_ioctl: if T::TO_USE.ioctl {
            Some(unlocked_ioctl_callback::<T>)
        } else {
            None
        },
        write_iter: if T::TO_USE.write_iter {
            Some(write_iter_callback::<T>)
        } else {
            None
        },
    };

    /// Builds an instance of [`struct file_operations`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the adapter is compatible with the way the device is registered.
    pub(crate) const unsafe fn build() -> &'static bindings::file_operations {
        &Self::VTABLE
    }
}

/// Represents which fields of [`struct file_operations`] should be populated with pointers.
pub struct ToUse {
    /// The `read` field of [`struct file_operations`].
    pub read: bool,

    /// The `read_iter` field of [`struct file_operations`].
    pub read_iter: bool,

    /// The `write` field of [`struct file_operations`].
    pub write: bool,

    /// The `write_iter` field of [`struct file_operations`].
    pub write_iter: bool,

    /// The `llseek` field of [`struct file_operations`].
    pub seek: bool,

    /// The `unlocked_ioctl` field of [`struct file_operations`].
    pub ioctl: bool,

    /// The `compat_ioctl` field of [`struct file_operations`].
    pub compat_ioctl: bool,

    /// The `fsync` field of [`struct file_operations`].
    pub fsync: bool,

    /// The `mmap` field of [`struct file_operations`].
    pub mmap: bool,

    /// The `poll` field of [`struct file_operations`].
    pub poll: bool,
}

/// A constant version where all values are to set to `false`, that is, all supported fields will
/// be set to null pointers.
pub const USE_NONE: ToUse = ToUse {
    read: false,
    read_iter: false,
    write: false,
    write_iter: false,
    seek: false,
    ioctl: false,
    compat_ioctl: false,
    fsync: false,
    mmap: false,
    poll: false,
};

/// Defines the [`FileOperations::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_file_operations {
    () => {
        const TO_USE: $crate::file_operations::ToUse = $crate::file_operations::USE_NONE;
    };
    ($($i:ident),+) => {
        const TO_USE: kernel::file_operations::ToUse =
            $crate::file_operations::ToUse {
                $($i: true),+ ,
                ..$crate::file_operations::USE_NONE
            };
    };
}

/// Allows the handling of ioctls defined with the `_IO`, `_IOR`, `_IOW`, and `_IOWR` macros.
///
/// For each macro, there is a handler function that takes the appropriate types as arguments.
pub trait IoctlHandler: Sync {
    /// Handles ioctls defined with the `_IO` macro, that is, with no buffer as argument.
    fn pure(&self, _file: &File, _cmd: u32, _arg: usize) -> Result<i32> {
        Err(Error::EINVAL)
    }

    /// Handles ioctls defined with the `_IOR` macro, that is, with an output buffer provided as
    /// argument.
    fn read(&self, _file: &File, _cmd: u32, _writer: &mut UserSlicePtrWriter) -> Result<i32> {
        Err(Error::EINVAL)
    }

    /// Handles ioctls defined with the `_IOW` macro, that is, with an input buffer provided as
    /// argument.
    fn write(&self, _file: &File, _cmd: u32, _reader: &mut UserSlicePtrReader) -> Result<i32> {
        Err(Error::EINVAL)
    }

    /// Handles ioctls defined with the `_IOWR` macro, that is, with a buffer for both input and
    /// output provided as argument.
    fn read_write(&self, _file: &File, _cmd: u32, _data: UserSlicePtr) -> Result<i32> {
        Err(Error::EINVAL)
    }
}

/// Represents an ioctl command.
///
/// It can use the components of an ioctl command to dispatch ioctls using
/// [`IoctlCommand::dispatch`].
pub struct IoctlCommand {
    cmd: u32,
    arg: usize,
    user_slice: Option<UserSlicePtr>,
}

impl IoctlCommand {
    /// Constructs a new [`IoctlCommand`].
    fn new(cmd: u32, arg: usize) -> Self {
        let size = (cmd >> bindings::_IOC_SIZESHIFT) & bindings::_IOC_SIZEMASK;

        // SAFETY: We only create one instance of the user slice per ioctl call, so TOCTOU issues
        // are not possible.
        let user_slice = Some(unsafe { UserSlicePtr::new(arg as _, size as _) });
        Self {
            cmd,
            arg,
            user_slice,
        }
    }

    /// Dispatches the given ioctl to the appropriate handler based on the value of the command. It
    /// also creates a [`UserSlicePtr`], [`UserSlicePtrReader`], or [`UserSlicePtrWriter`]
    /// depending on the direction of the buffer of the command.
    ///
    /// It is meant to be used in implementations of [`FileOperations::ioctl`] and
    /// [`FileOperations::compat_ioctl`].
    pub fn dispatch<T: IoctlHandler>(&mut self, handler: &T, file: &File) -> Result<i32> {
        let dir = (self.cmd >> bindings::_IOC_DIRSHIFT) & bindings::_IOC_DIRMASK;
        if dir == bindings::_IOC_NONE {
            return handler.pure(file, self.cmd, self.arg);
        }

        let data = self.user_slice.take().ok_or(Error::EINVAL)?;
        const READ_WRITE: u32 = bindings::_IOC_READ | bindings::_IOC_WRITE;
        match dir {
            bindings::_IOC_WRITE => handler.write(file, self.cmd, &mut data.reader()),
            bindings::_IOC_READ => handler.read(file, self.cmd, &mut data.writer()),
            READ_WRITE => handler.read_write(file, self.cmd, data),
            _ => Err(Error::EINVAL),
        }
    }

    /// Returns the raw 32-bit value of the command and the ptr-sized argument.
    pub fn raw(&self) -> (u32, usize) {
        (self.cmd, self.arg)
    }
}

/// Trait for extracting file open arguments from kernel data structures.
///
/// This is meant to be implemented by registration managers.
pub trait FileOpenAdapter {
    /// The type of argument this adapter extracts.
    type Arg;

    /// Converts untyped data stored in [`struct inode`] and [`struct file`] (when [`struct
    /// file_operations::open`] is called) into the given type. For example, for `miscdev`
    /// devices, a pointer to the registered [`struct miscdev`] is stored in [`struct
    /// file::private_data`].
    ///
    /// # Safety
    ///
    /// This function must be called only when [`struct file_operations::open`] is being called for
    /// a file that was registered by the implementer.
    // TODO: are these pointers guaranteed to be `NonNull`?
    unsafe fn convert(_inode: *mut bindings::inode, _file: *mut bindings::file)
        -> *const Self::Arg;
}

/// Trait for implementers of kernel files.
///
/// In addition to the methods in [`FileOperations`], implementers must also provide
/// [`FileOpener::open`] with a customised argument. This allows a single implementation of
/// [`FileOperations`] to be used for different types of registrations, for example, `miscdev` and
/// `chrdev`.
pub trait FileOpener<T: ?Sized>: FileOperations {
    /// Creates a new instance of this file.
    ///
    /// Corresponds to the `open` function pointer in `struct file_operations`.
    fn open(context: &T) -> Result<Self::Wrapper>;
}

impl<T: FileOperations<Wrapper = Box<T>> + Default> FileOpener<()> for T {
    fn open(_: &()) -> Result<Self::Wrapper> {
        Ok(Box::try_new(T::default())?)
    }
}

/// Corresponds to the kernel's `struct file_operations`.
///
/// You implement this trait whenever you would create a `struct file_operations`.
///
/// File descriptors may be used from multiple threads/processes concurrently, so your type must be
/// [`Sync`]. It must also be [`Send`] because [`FileOperations::release`] will be called from the
/// thread that decrements that associated file's refcount to zero.
pub trait FileOperations: Send + Sync + Sized {
    /// The methods to use to populate [`struct file_operations`].
    const TO_USE: ToUse;

    /// The pointer type that will be used to hold ourselves.
    type Wrapper: PointerWrapper = Box<Self>;

    /// Cleans up after the last reference to the file goes away.
    ///
    /// Note that the object is moved, so it will be freed automatically unless the implementation
    /// moves it elsewhere.
    ///
    /// Corresponds to the `release` function pointer in `struct file_operations`.
    fn release(_obj: Self::Wrapper, _file: &File) {}

    /// Reads data from this file to the caller's buffer.
    ///
    /// Corresponds to the `read` and `read_iter` function pointers in `struct file_operations`.
    fn read<T: IoBufferWriter>(&self, _file: &File, _data: &mut T, _offset: u64) -> Result<usize> {
        Err(Error::EINVAL)
    }

    /// Writes data from the caller's buffer to this file.
    ///
    /// Corresponds to the `write` and `write_iter` function pointers in `struct file_operations`.
    fn write<T: IoBufferReader>(&self, _file: &File, _data: &mut T, _offset: u64) -> Result<usize> {
        Err(Error::EINVAL)
    }

    /// Changes the position of the file.
    ///
    /// Corresponds to the `llseek` function pointer in `struct file_operations`.
    fn seek(&self, _file: &File, _offset: SeekFrom) -> Result<u64> {
        Err(Error::EINVAL)
    }

    /// Performs IO control operations that are specific to the file.
    ///
    /// Corresponds to the `unlocked_ioctl` function pointer in `struct file_operations`.
    fn ioctl(&self, _file: &File, _cmd: &mut IoctlCommand) -> Result<i32> {
        Err(Error::EINVAL)
    }

    /// Performs 32-bit IO control operations on that are specific to the file on 64-bit kernels.
    ///
    /// Corresponds to the `compat_ioctl` function pointer in `struct file_operations`.
    fn compat_ioctl(&self, _file: &File, _cmd: &mut IoctlCommand) -> Result<i32> {
        Err(Error::EINVAL)
    }

    /// Syncs pending changes to this file.
    ///
    /// Corresponds to the `fsync` function pointer in `struct file_operations`.
    fn fsync(&self, _file: &File, _start: u64, _end: u64, _datasync: bool) -> Result<u32> {
        Err(Error::EINVAL)
    }

    /// Maps areas of the caller's virtual memory with device/file memory.
    ///
    /// Corresponds to the `mmap` function pointer in `struct file_operations`.
    /// TODO: wrap `vm_area_struct` so that we don't have to expose it.
    fn mmap(&self, _file: &File, _vma: &mut bindings::vm_area_struct) -> Result {
        Err(Error::EINVAL)
    }

    /// Checks the state of the file and optionally registers for notification when the state
    /// changes.
    ///
    /// Corresponds to the `poll` function pointer in `struct file_operations`.
    fn poll(&self, _file: &File, _table: &PollTable) -> Result<u32> {
        Ok(bindings::POLLIN | bindings::POLLOUT | bindings::POLLRDNORM | bindings::POLLWRNORM)
    }
}
