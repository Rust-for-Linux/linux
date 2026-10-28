// SPDX-License-Identifier: GPL-2.0

//! Errors for the [`Vec`] type.

use crate::{
    fmt,
    prelude::*, //
};

/// Error type for a push that fails because there is no spare capacity.
pub struct PushError<T>(pub T);

impl<T> fmt::Debug for PushError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Not enough capacity")
    }
}

impl<T> From<PushError<T>> for Error {
    #[inline]
    fn from(_: PushError<T>) -> Error {
        // Returning ENOMEM isn't appropriate because the system is not out of memory. The vector
        // is just full and we are refusing to resize it.
        EINVAL
    }
}

/// Error type for [`Vec::try_push_init`].
pub enum TryPushInitError<I, E> {
    /// The allocation failed. Hand the initializer back.
    AllocError(I),
    /// The initializer failed.
    InitError(E),
}

impl<I, E> fmt::Debug for TryPushInitError<I, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TryPushInitError::AllocError(_) => write!(f, "Failed to allocate"),
            TryPushInitError::InitError(_) => write!(f, "Initializer failed"),
        }
    }
}

impl<I, E> From<TryPushInitError<I, E>> for Error
where
    Error: From<E>,
{
    #[inline]
    fn from(e: TryPushInitError<I, E>) -> Error {
        match e {
            TryPushInitError::AllocError(_) => ENOMEM,
            TryPushInitError::InitError(e) => Error::from(e),
        }
    }
}

/// Error type for [`Vec::remove`].
pub struct RemoveError;

impl fmt::Debug for RemoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Index out of bounds")
    }
}

impl From<RemoveError> for Error {
    #[inline]
    fn from(_: RemoveError) -> Error {
        EINVAL
    }
}

/// Error type for [`Vec::insert_within_capacity`].
pub enum InsertError<T> {
    /// The value could not be inserted because the index is out of bounds.
    IndexOutOfBounds(T),
    /// The value could not be inserted because the vector is out of capacity.
    OutOfCapacity(T),
}

impl<T> fmt::Debug for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InsertError::IndexOutOfBounds(_) => write!(f, "Index out of bounds"),
            InsertError::OutOfCapacity(_) => write!(f, "Not enough capacity"),
        }
    }
}

impl<T> From<InsertError<T>> for Error {
    #[inline]
    fn from(_: InsertError<T>) -> Error {
        EINVAL
    }
}
