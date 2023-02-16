// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM MM range allocator
//!
//! C header: [`include/linux/drm/drm_mm.h`](../../../../include/linux/drm/drm_mm.h)

use crate::{
    bindings,
    error::{to_result, Result},
    str::CStr,
    sync::{Arc, LockClassKey, LockIniter, Mutex, UniqueArc},
    types::Opaque,
};

use alloc::boxed::Box;

use core::{
    marker::{PhantomData, PhantomPinned},
    ops::Deref,
    pin::Pin,
};

/// Type alias representing a DRM MM node.
pub type Node<A, T> = Pin<Box<NodeData<A, T>>>;

/// Trait which must be implemented by the inner allocator state type provided by the user.
pub trait AllocInner<T> {
    /// Notification that a node was dropped from the allocator.
    fn drop_object(&mut self, _start: u64, _size: u64, _color: usize, _object: &mut T) {}
}

impl<T> AllocInner<T> for () {}

/// Wrapper type for a `struct drm_mm` plus user AllocInner object.
///
/// # Invariants
/// The `drm_mm` struct is valid and initialized.
struct MmInner<A: AllocInner<T>, T>(Opaque<bindings::drm_mm>, A, PhantomData<T>);

/// Represents a single allocated node in the MM allocator
pub struct NodeData<A: AllocInner<T>, T> {
    node: bindings::drm_mm_node,
    mm: Arc<Mutex<MmInner<A, T>>>,
    valid: bool,
    /// A drm_mm_node needs to be pinned because nodes reference each other in a linked list.
    _pin: PhantomPinned,
    inner: T,
}

// SAFETY: Allocator ops take the mutex, and there are no mutable actions on the node.
unsafe impl<A: Send + AllocInner<T>, T: Send> Send for NodeData<A, T> {}
unsafe impl<A: Send + AllocInner<T>, T: Sync> Sync for NodeData<A, T> {}

/// Available MM node insertion modes
#[repr(u32)]
pub enum InsertMode {
    /// Search for the smallest hole (within the search range) that fits the desired node.
    ///
    /// Allocates the node from the bottom of the found hole.
    Best = bindings::drm_mm_insert_mode_DRM_MM_INSERT_BEST,

    /// Search for the lowest hole (address closest to 0, within the search range) that fits the
    /// desired node.
    ///
    /// Allocates the node from the bottom of the found hole.
    Low = bindings::drm_mm_insert_mode_DRM_MM_INSERT_LOW,

    /// Search for the highest hole (address closest to U64_MAX, within the search range) that fits
    /// the desired node.
    ///
    /// Allocates the node from the top of the found hole. The specified alignment for the node is
    /// applied to the base of the node (`Node.start()`).
    High = bindings::drm_mm_insert_mode_DRM_MM_INSERT_HIGH,

    /// Search for the most recently evicted hole (within the search range) that fits the desired
    /// node. This is appropriate for use immediately after performing an eviction scan and removing
    /// the selected nodes to form a hole.
    ///
    /// Allocates the node from the bottom of the found hole.
    Evict = bindings::drm_mm_insert_mode_DRM_MM_INSERT_EVICT,
}

/// A clonable, interlocked reference to the allocator state.
///
/// This is useful to perform actions on the user-supplied `AllocInner<T>` type given just a Node,
/// without immediately taking the lock.
#[derive(Clone)]
pub struct InnerRef<A: AllocInner<T>, T>(Arc<Mutex<MmInner<A, T>>>);

impl<A: AllocInner<T>, T> InnerRef<A, T> {
    /// Operate on the user `AllocInner<T>` implementation, taking the lock.
    pub fn with<RetVal>(&self, cb: impl FnOnce(&mut A) -> RetVal) -> RetVal {
        let mut l = self.0.lock();
        cb(&mut l.1)
    }
}

impl<A: AllocInner<T>, T> NodeData<A, T> {
    /// Returns the color of the node (an opaque value)
    pub fn color(&self) -> usize {
        self.node.color as usize
    }

    /// Returns the start address of the node
    pub fn start(&self) -> u64 {
        self.node.start
    }

    /// Returns the size of the node in bytes
    pub fn size(&self) -> u64 {
        self.node.size
    }

    /// Operate on the user `AllocInner<T>` implementation associated with this node's allocator.
    pub fn with_inner<RetVal>(&self, cb: impl FnOnce(&mut A) -> RetVal) -> RetVal {
        let mut l = self.mm.lock();
        cb(&mut l.1)
    }

    /// Return a clonable, detached reference to the allocator inner data.
    pub fn alloc_ref(&self) -> InnerRef<A, T> {
        InnerRef(self.mm.clone())
    }

    /// Return a mutable reference to the inner data.
    pub fn inner_mut(self: Pin<&mut Self>) -> &mut T {
        // SAFETY: This is okay because inner is not structural
        unsafe { &mut self.get_unchecked_mut().inner }
    }
}

impl<A: AllocInner<T>, T> Deref for NodeData<A, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A: AllocInner<T>, T> Drop for NodeData<A, T> {
    fn drop(&mut self) {
        if self.valid {
            let mut guard = self.mm.lock();

            // Inform the user allocator that a node is being dropped.
            guard
                .1
                .drop_object(self.start(), self.size(), self.color(), &mut self.inner);
            // SAFETY: The MM lock is still taken, so we can safely remove the node.
            unsafe { bindings::drm_mm_remove_node(&mut self.node) };
        }
    }
}

/// An instance of a DRM MM range allocator.
pub struct Allocator<A: AllocInner<T>, T> {
    mm: Arc<Mutex<MmInner<A, T>>>,
    _p: PhantomData<T>,
}

impl<A: AllocInner<T>, T> Allocator<A, T> {
    /// Create a new range allocator for the given start and size range of addresses.
    ///
    /// The user may optionally provide an inner object representing allocator state, which will
    /// be protected by the same lock. If not required, `()` can be used.
    pub fn new(
        start: u64,
        size: u64,
        inner: A,
        name: &'static CStr,
        lock_key: &'static LockClassKey,
    ) -> Result<Allocator<A, T>> {
        // SAFETY: We call `Mutex::init_lock` below.
        let mut mm: Pin<UniqueArc<Mutex<MmInner<A, T>>>> = UniqueArc::try_new(unsafe {
            Mutex::new(MmInner(Opaque::uninit(), inner, PhantomData))
        })?
        .into();

        mm.as_mut().init_lock(name, lock_key);

        unsafe {
            // SAFETY: The Opaque instance provides a valid pointer, and it is initialized after
            // this call.
            bindings::drm_mm_init(mm.lock().0.get(), start, size);
        }

        Ok(Allocator {
            mm: mm.into(),
            _p: PhantomData,
        })
    }

    /// Insert a new node into the allocator of a given size.
    ///
    /// `node` is the user `T` type data to store into the node.
    pub fn insert_node(&mut self, node: T, size: u64) -> Result<Node<A, T>> {
        self.insert_node_generic(node, size, 0, 0, InsertMode::Best)
    }

    /// Insert a new node into the allocator of a given size, with configurable alignment,
    /// color, and insertion mode.
    ///
    /// `node` is the user `T` type data to store into the node.
    pub fn insert_node_generic(
        &mut self,
        node: T,
        size: u64,
        alignment: u64,
        color: usize,
        mode: InsertMode,
    ) -> Result<Node<A, T>> {
        self.insert_node_in_range(node, size, alignment, color, 0, u64::MAX, mode)
    }

    /// Insert a new node into the allocator of a given size, with configurable alignment,
    /// color, insertion mode, and sub-range to allocate from.
    ///
    /// `node` is the user `T` type data to store into the node.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_node_in_range(
        &mut self,
        node: T,
        size: u64,
        alignment: u64,
        color: usize,
        start: u64,
        end: u64,
        mode: InsertMode,
    ) -> Result<Node<A, T>> {
        let mut mm_node = Box::try_new(NodeData {
            // SAFETY: This C struct should be zero-initialized.
            node: unsafe { core::mem::zeroed() },
            valid: false,
            inner: node,
            mm: self.mm.clone(),
            _pin: PhantomPinned,
        })?;

        let guard = self.mm.lock();
        // SAFETY: We hold the lock and all pointers are valid.
        to_result(unsafe {
            bindings::drm_mm_insert_node_in_range(
                guard.0.get(),
                &mut mm_node.node,
                size,
                alignment,
                color as core::ffi::c_ulong,
                start,
                end,
                mode as u32,
            )
        })?;

        mm_node.valid = true;

        Ok(Pin::from(mm_node))
    }

    /// Insert a node into the allocator at a fixed start address.
    ///
    /// `node` is the user `T` type data to store into the node.
    pub fn reserve_node(
        &mut self,
        node: T,
        start: u64,
        size: u64,
        color: usize,
    ) -> Result<Node<A, T>> {
        let mut mm_node = Box::try_new(NodeData {
            // SAFETY: This C struct should be zero-initialized.
            node: unsafe { core::mem::zeroed() },
            valid: false,
            inner: node,
            mm: self.mm.clone(),
            _pin: PhantomPinned,
        })?;

        mm_node.node.start = start;
        mm_node.node.size = size;
        mm_node.node.color = color as core::ffi::c_ulong;

        let guard = self.mm.lock();
        // SAFETY: We hold the lock and all pointers are valid.
        to_result(unsafe { bindings::drm_mm_reserve_node(guard.0.get(), &mut mm_node.node) })?;

        mm_node.valid = true;

        Ok(Pin::from(mm_node))
    }

    /// Operate on the inner user type `A`, taking the allocator lock
    pub fn with_inner<RetVal>(&self, cb: impl FnOnce(&mut A) -> RetVal) -> RetVal {
        let mut guard = self.mm.lock();
        cb(&mut guard.1)
    }
}

impl<A: AllocInner<T>, T> Drop for MmInner<A, T> {
    fn drop(&mut self) {
        // SAFETY: If the MmInner is dropped then all nodes are gone (since they hold references),
        // so it is safe to tear down the allocator.
        unsafe {
            bindings::drm_mm_takedown(self.0.get());
        }
    }
}

// MmInner is safely Send if the AllocInner user type is Send.
unsafe impl<A: Send + AllocInner<T>, T> Send for MmInner<A, T> {}
