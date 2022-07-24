// SPDX-License-Identifier: GPL-2.0

//! Kernel errors.
//!
//! C header: [`include/uapi/asm-generic/errno-base.h`](../../../include/uapi/asm-generic/errno-base.h)

use crate::bindings;
use crate::str::CStr;
use alloc::{
    alloc::{AllocError, LayoutError},
    collections::TryReserveError,
};
use core::convert::From;
use core::fmt;
use core::num::TryFromIntError;
use core::str::{self, Utf8Error};

/// Contains the C-compatible error codes.
pub mod code {
    macro_rules! declare_err {
        ($err:tt $(,)? $($doc:expr),+) => {
            $(
            #[doc = $doc]
            )*
            pub const $err: super::Error = super::Error(-(crate::bindings::$err as i32));
        };
    }

    declare_err!(EPERM, "Operation not permitted.");

    declare_err!(ENOENT, "No such file or directory.");

    declare_err!(ESRCH, "No such process.");

    declare_err!(EINTR, "Interrupted system call.");

    declare_err!(EIO, "I/O error.");

    declare_err!(ENXIO, "No such device or address.");

    declare_err!(E2BIG, "Argument list too long.");

    declare_err!(ENOEXEC, "Exec format error.");

    declare_err!(EBADF, "Bad file number.");

    declare_err!(ECHILD, "Exec format error.");

    declare_err!(EAGAIN, "Try again.");

    declare_err!(ENOMEM, "Out of memory.");

    declare_err!(EACCES, "Permission denied.");

    declare_err!(EFAULT, "Bad address.");

    declare_err!(ENOTBLK, "Block device required.");

    declare_err!(EBUSY, "Device or resource busy.");

    declare_err!(EEXIST, "File exists.");

    declare_err!(EXDEV, "Cross-device link.");

    declare_err!(ENODEV, "No such device.");

    declare_err!(ENOTDIR, "Not a directory.");

    declare_err!(EISDIR, "Is a directory.");

    declare_err!(EINVAL, "Invalid argument.");

    declare_err!(ENFILE, "File table overflow.");

    declare_err!(EMFILE, "Too many open files.");

    declare_err!(ENOTTY, "Not a typewriter.");

    declare_err!(ETXTBSY, "Text file busy.");

    declare_err!(EFBIG, "File too large.");

    declare_err!(ENOSPC, "No space left on device.");

    declare_err!(ESPIPE, "Illegal seek.");

    declare_err!(EROFS, "Read-only file system.");

    declare_err!(EMLINK, "Too many links.");

    declare_err!(EPIPE, "Broken pipe.");

    declare_err!(EDOM, "Math argument out of domain of func.");

    declare_err!(ERANGE, "Math result not representable.");

    declare_err!(EDEADLK, "Resource deadlock would occur");

    declare_err!(ENAMETOOLONG, "File name too long");

    declare_err!(ENOLCK, "No record locks available");

    declare_err!(
        ENOSYS,
        "Invalid system call number.",
        "",
        "This error code is special: arch syscall entry code will return",
        "[`ENOSYS`] if users try to call a syscall that doesn't exist.",
        "To keep failures of syscalls that really do exist distinguishable from",
        "failures due to attempts to use a nonexistent syscall, syscall",
        "implementations should refrain from returning [`ENOSYS`]."
    );

    declare_err!(ENOTEMPTY, "Directory not empty.");

    declare_err!(ELOOP, "Too many symbolic links encountered.");

    declare_err!(EWOULDBLOCK, "Operation would block.");

    declare_err!(ENOMSG, "No message of desired type.");

    declare_err!(EIDRM, "Identifier removed.");

    declare_err!(ECHRNG, "Channel number out of range.");

    declare_err!(EL2NSYNC, "Level 2 not synchronized.");

    declare_err!(EL3HLT, "Level 3 halted.");

    declare_err!(EL3RST, "Level 3 reset.");

    declare_err!(ELNRNG, "Link number out of range.");

    declare_err!(EUNATCH, "Protocol driver not attached.");

    declare_err!(ENOCSI, "No CSI structure available.");

    declare_err!(EL2HLT, "Level 2 halted.");

    declare_err!(EBADE, "Invalid exchange.");

    declare_err!(EBADR, "Invalid request descriptor.");

    declare_err!(EXFULL, "Exchange full.");

    declare_err!(ENOANO, "No anode.");

    declare_err!(EBADRQC, "Invalid request code.");

    declare_err!(EBADSLT, "Invalid slot.");

    declare_err!(EDEADLOCK, "Resource deadlock would occur.");

    declare_err!(EBFONT, "Bad font file format.");

    declare_err!(ENOSTR, "Device not a stream.");

    declare_err!(ENODATA, "No data available.");

    declare_err!(ETIME, "Timer expired.");

    declare_err!(ENOSR, "Out of streams resources.");

    declare_err!(ENONET, "Machine is not on the network.");

    declare_err!(ENOPKG, "Package not installed.");

    declare_err!(EREMOTE, "Object is remote.");

    declare_err!(ENOLINK, "Link has been severed.");

    declare_err!(EADV, "Advertise error.");

    declare_err!(ESRMNT, "Srmount error.");

    declare_err!(ECOMM, "Communication error on send.");

    declare_err!(EPROTO, "Protocol error.");

    declare_err!(EMULTIHOP, "Multihop attempted.");

    declare_err!(EDOTDOT, "RFS specific error.");

    declare_err!(EBADMSG, "Not a data message.");

    declare_err!(EOVERFLOW, "Value too large for defined data type.");

    declare_err!(ENOTUNIQ, "Name not unique on network.");

    declare_err!(EBADFD, "File descriptor in bad state.");

    declare_err!(EREMCHG, "Remote address changed.");

    declare_err!(ELIBACC, "Can not access a needed shared library.");

    declare_err!(ELIBBAD, "Accessing a corrupted shared library.");

    declare_err!(ELIBSCN, ".lib section in a.out corrupted.");

    declare_err!(ELIBMAX, "Attempting to link in too many shared libraries.");

    declare_err!(ELIBEXEC, "Cannot exec a shared library directly.");

    declare_err!(EILSEQ, "Illegal byte sequence.");

    declare_err!(ERESTART, "Interrupted system call should be restarted.");

    declare_err!(ESTRPIPE, "Streams pipe error.");

    declare_err!(EUSERS, "Too many users.");

    declare_err!(ENOTSOCK, "Socket operation on non-socket.");

    declare_err!(EDESTADDRREQ, "Destination address required.");

    declare_err!(EMSGSIZE, "Message too long.");

    declare_err!(EPROTOTYPE, "Protocol wrong type for socket.");

    declare_err!(ENOPROTOOPT, "Protocol not available.");

    declare_err!(EPROTONOSUPPORT, "Protocol not supported.");

    declare_err!(ESOCKTNOSUPPORT, "Socket type not supported.");

    declare_err!(EOPNOTSUPP, "Operation not supported on transport endpoint.");

    declare_err!(EPFNOSUPPORT, "Protocol family not supported.");

    declare_err!(EAFNOSUPPORT, "Address family not supported by protocol.");

    declare_err!(EADDRINUSE, "Address already in use.");

    declare_err!(EADDRNOTAVAIL, "Cannot assign requested address.");

    declare_err!(ENETDOWN, "Network is down.");

    declare_err!(ENETUNREACH, "Network is unreachable.");

    declare_err!(ENETRESET, "Network dropped connection because of reset.");

    declare_err!(ECONNABORTED, "Software caused connection abort.");

    declare_err!(ECONNRESET, "Connection reset by peer.");

    declare_err!(ENOBUFS, "No buffer space available.");

    declare_err!(EISCONN, "Transport endpoint is already connected.");

    declare_err!(ENOTCONN, "Transport endpoint is not connected.");

    declare_err!(ESHUTDOWN, "Cannot send after transport endpoint shutdown.");

    declare_err!(ETOOMANYREFS, "Too many references: cannot splice.");

    declare_err!(ETIMEDOUT, "Connection timed out.");

    declare_err!(ECONNREFUSED, "Connection refused.");

    declare_err!(EHOSTDOWN, "Host is down.");

    declare_err!(EHOSTUNREACH, "No route to host.");

    declare_err!(EALREADY, "Operation already in progress.");

    declare_err!(EINPROGRESS, "Operation now in progress.");

    declare_err!(ESTALE, "Stale file handle.");

    declare_err!(EUCLEAN, "Structure needs cleaning.");

    declare_err!(ENOTNAM, "Not a XENIX named type file.");

    declare_err!(ENAVAIL, "No XENIX semaphores available.");

    declare_err!(EISNAM, "Is a named type file.");

    declare_err!(EREMOTEIO, "Remote I/O error.");

    declare_err!(EDQUOT, "Quota exceeded.");

    declare_err!(ENOMEDIUM, "No medium found.");

    declare_err!(EMEDIUMTYPE, "Wrong medium type.");

    declare_err!(ECANCELED, "Operation Canceled.");

    declare_err!(ENOKEY, "Required key not available.");

    declare_err!(EKEYEXPIRED, "Key has expired.");

    declare_err!(EKEYREVOKED, "Key has been revoked.");

    declare_err!(EKEYREJECTED, "Key was rejected by service.");

    declare_err!(EOWNERDEAD, "Owner died.", "", "For robust mutexes.");

    declare_err!(ENOTRECOVERABLE, "State not recoverable.");

    declare_err!(ERFKILL, "Operation not possible due to RF-kill.");

    declare_err!(EHWPOISON, "Memory page has hardware error.");

    declare_err!(ERESTARTSYS, "Restart the system call.");

    declare_err!(ENOTSUPP, "Operation is not supported.");

    declare_err!(ENOPARAM, "Parameter not supported.");
}

/// Generic integer kernel error.
///
/// The kernel defines a set of integer generic error codes based on C and
/// POSIX ones. These codes may have a more specific meaning in some contexts.
///
/// # Invariants
///
/// The value is a valid `errno` (i.e. `>= -MAX_ERRNO && < 0`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error(core::ffi::c_int);

impl Error {
    /// Creates an [`Error`] from a kernel error code.
    ///
    /// It is a bug to pass an out-of-range `errno`. `EINVAL` would
    /// be returned in such a case.
    pub(crate) fn from_kernel_errno(errno: core::ffi::c_int) -> Error {
        if errno < -(bindings::MAX_ERRNO as i32) || errno >= 0 {
            // TODO: Make it a `WARN_ONCE` once available.
            crate::pr_warn!(
                "attempted to create `Error` with out of range `errno`: {}",
                errno
            );
            return code::EINVAL;
        }

        // INVARIANT: The check above ensures the type invariant
        // will hold.
        Error(errno)
    }

    /// Creates an [`Error`] from a kernel error code.
    ///
    /// # Safety
    ///
    /// `errno` must be within error code range (i.e. `>= -MAX_ERRNO && < 0`).
    pub(crate) unsafe fn from_kernel_errno_unchecked(errno: core::ffi::c_int) -> Error {
        // INVARIANT: The contract ensures the type invariant
        // will hold.
        Error(errno)
    }

    /// Returns the kernel error code.
    pub fn to_kernel_errno(self) -> core::ffi::c_int {
        self.0
    }

    /// Returns a string representing the error, if one exists.
    #[cfg(not(testlib))]
    pub fn name(&self) -> Option<&'static CStr> {
        // SAFETY: Just an FFI call, there are no extra safety requirements.
        let ptr = unsafe { bindings::errname(-self.0) };
        if ptr.is_null() {
            None
        } else {
            // SAFETY: The string returned by `errname` is static and `NUL`-terminated.
            Some(unsafe { CStr::from_char_ptr(ptr) })
        }
    }

    /// Returns a string representing the error, if one exists.
    ///
    /// When `testlib` is configured, this always returns `None` to avoid the dependency on a
    /// kernel function so that tests that use this (e.g., by calling [`Result::unwrap`]) can still
    /// run in userspace.
    #[cfg(testlib)]
    pub fn name(&self) -> Option<&'static CStr> {
        None
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            // Print out number if no name can be found.
            None => f.debug_tuple("Error").field(&-self.0).finish(),
            // SAFETY: These strings are ASCII-only.
            Some(name) => f
                .debug_tuple(unsafe { str::from_utf8_unchecked(name) })
                .finish(),
        }
    }
}

impl From<TryFromIntError> for Error {
    fn from(_: TryFromIntError) -> Error {
        code::EINVAL
    }
}

impl From<Utf8Error> for Error {
    fn from(_: Utf8Error) -> Error {
        code::EINVAL
    }
}

impl From<TryReserveError> for Error {
    fn from(_: TryReserveError) -> Error {
        code::ENOMEM
    }
}

impl From<LayoutError> for Error {
    fn from(_: LayoutError) -> Error {
        code::ENOMEM
    }
}

impl From<core::fmt::Error> for Error {
    fn from(_: core::fmt::Error) -> Error {
        code::EINVAL
    }
}

impl From<core::convert::Infallible> for Error {
    fn from(e: core::convert::Infallible) -> Error {
        match e {}
    }
}

/// A [`Result`] with an [`Error`] error type.
///
/// To be used as the return type for functions that may fail.
///
/// # Error codes in C and Rust
///
/// In C, it is common that functions indicate success or failure through
/// their return value; modifying or returning extra data through non-`const`
/// pointer parameters. In particular, in the kernel, functions that may fail
/// typically return an `int` that represents a generic error code. We model
/// those as [`Error`].
///
/// In Rust, it is idiomatic to model functions that may fail as returning
/// a [`Result`]. Since in the kernel many functions return an error code,
/// [`Result`] is a type alias for a [`core::result::Result`] that uses
/// [`Error`] as its error type.
///
/// Note that even if a function does not return anything when it succeeds,
/// it should still be modeled as returning a `Result` rather than
/// just an [`Error`].
pub type Result<T = ()> = core::result::Result<T, Error>;

impl From<AllocError> for Error {
    fn from(_: AllocError) -> Error {
        code::ENOMEM
    }
}

// # Invariant: `-bindings::MAX_ERRNO` fits in an `i16`.
crate::static_assert!(bindings::MAX_ERRNO <= -(i16::MIN as i32) as u32);

pub(crate) fn from_kernel_result_helper<T>(r: Result<T>) -> T
where
    T: From<i16>,
{
    match r {
        Ok(v) => v,
        // NO-OVERFLOW: negative `errno`s are no smaller than `-bindings::MAX_ERRNO`,
        // `-bindings::MAX_ERRNO` fits in an `i16` as per invariant above,
        // therefore a negative `errno` always fits in an `i16` and will not overflow.
        Err(e) => T::from(e.to_kernel_errno() as i16),
    }
}

/// Transforms a [`crate::error::Result<T>`] to a kernel C integer result.
///
/// This is useful when calling Rust functions that return [`crate::error::Result<T>`]
/// from inside `extern "C"` functions that need to return an integer
/// error result.
///
/// `T` should be convertible to an `i16` via `From<i16>`.
///
/// # Examples
///
/// ```ignore
/// # use kernel::from_kernel_result;
/// # use kernel::bindings;
/// unsafe extern "C" fn probe_callback(
///     pdev: *mut bindings::platform_device,
/// ) -> core::ffi::c_int {
///     from_kernel_result! {
///         let ptr = devm_alloc(pdev)?;
///         bindings::platform_set_drvdata(pdev, ptr);
///         Ok(0)
///     }
/// }
/// ```
macro_rules! from_kernel_result {
    ($($tt:tt)*) => {{
        $crate::error::from_kernel_result_helper((|| {
            $($tt)*
        })())
    }};
}

pub(crate) use from_kernel_result;

/// Transform a kernel "error pointer" to a normal pointer.
///
/// Some kernel C API functions return an "error pointer" which optionally
/// embeds an `errno`. Callers are supposed to check the returned pointer
/// for errors. This function performs the check and converts the "error pointer"
/// to a normal pointer in an idiomatic fashion.
///
/// # Examples
///
/// ```ignore
/// # use kernel::from_kernel_err_ptr;
/// # use kernel::bindings;
/// fn devm_platform_ioremap_resource(
///     pdev: &mut PlatformDevice,
///     index: u32,
/// ) -> Result<*mut core::ffi::c_void> {
///     // SAFETY: FFI call.
///     unsafe {
///         from_kernel_err_ptr(bindings::devm_platform_ioremap_resource(
///             pdev.to_ptr(),
///             index,
///         ))
///     }
/// }
/// ```
// TODO: Remove `dead_code` marker once an in-kernel client is available.
#[allow(dead_code)]
pub(crate) fn from_kernel_err_ptr<T>(ptr: *mut T) -> Result<*mut T> {
    // CAST: Casting a pointer to `*const core::ffi::c_void` is always valid.
    let const_ptr: *const core::ffi::c_void = ptr.cast();
    // SAFETY: The FFI function does not deref the pointer.
    if unsafe { bindings::IS_ERR(const_ptr) } {
        // SAFETY: The FFI function does not deref the pointer.
        let err = unsafe { bindings::PTR_ERR(const_ptr) };
        // CAST: If `IS_ERR()` returns `true`,
        // then `PTR_ERR()` is guaranteed to return a
        // negative value greater-or-equal to `-bindings::MAX_ERRNO`,
        // which always fits in an `i16`, as per the invariant above.
        // And an `i16` always fits in an `i32`. So casting `err` to
        // an `i32` can never overflow, and is always valid.
        //
        // SAFETY: `IS_ERR()` ensures `err` is a
        // negative value greater-or-equal to `-bindings::MAX_ERRNO`.
        return Err(unsafe { Error::from_kernel_errno_unchecked(err as i32) });
    }
    Ok(ptr)
}

/// Converts an integer as returned by a C kernel function to an error if it's negative, and
/// `Ok(())` otherwise.
pub fn to_result(err: core::ffi::c_int) -> Result {
    if err < 0 {
        Err(Error::from_kernel_errno(err))
    } else {
        Ok(())
    }
}
