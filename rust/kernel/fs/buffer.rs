// SPDX-License-Identifier: GPL-2.0

//! File system buffers.
//!
//! C headers: [`include/linux/buffer_head.h`](../../../include/linux/buffer_head.h)

use crate::types::{ARef, AlwaysRefCounted, Opaque};
use core::ptr;

/// Wraps the kernel's `struct buffer_head`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `get_bh` ensures that the
/// allocation remains valid at least until the matching call to `put_bh`.
#[repr(transparent)]
pub struct Head(Opaque<bindings::buffer_head>);

// SAFETY: The type invariants guarantee that `INode` is always ref-counted.
unsafe impl AlwaysRefCounted for Head {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_bh(self.0.get()) };
    }

    unsafe fn dec_ref(obj: ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::put_bh(obj.cast().as_ptr()) }
    }
}

impl Head {
    /// Returns the block data associated with the given buffer head.
    pub fn data(&self) -> &[u8] {
        let h = self.0.get();
        // SAFETY: The existence of a shared reference guarantees that the buffer head is
        // available and so we can access its contents.
        unsafe { core::slice::from_raw_parts((*h).b_data.cast(), (*h).b_size) }
    }
}

/// A view of a buffer.
///
/// It may contain just a contiguous subset of the buffer.
pub struct View {
    head: ARef<Head>,
    offset: usize,
    size: usize,
}

impl View {
    pub(crate) fn new(head: ARef<Head>, offset: usize, size: usize) -> Self {
        Self { head, size, offset }
    }

    /// Returns the view of the buffer head.
    pub fn data(&self) -> &[u8] {
        &self.head.data()[self.offset..][..self.size]
    }
}
