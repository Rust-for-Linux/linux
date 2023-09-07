// SPDX-License-Identifier: GPL-2.0

//! Unified device property interface.
//!
//! C header: [`include/linux/property.h`](../../../../include/linux/property.h)

use crate::{bindings, device, ARef, AlwaysRefCounted};
use core::{cell::UnsafeCell, ptr};

/// Represents a `struct fwnode_handle *` part of a device's fwnode graph
///
/// # Invariants
///
/// The pointer is valid.
#[repr(transparent)]
pub struct Node(pub(crate) UnsafeCell<bindings::fwnode_handle>);

// SAFETY: The type invariants guarantee that `Node` is always ref-counted.
unsafe impl AlwaysRefCounted for Node {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::fwnode_handle_get(self.0.get()) };
    }

    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::fwnode_handle_put(obj.cast().as_ptr()) }
    }
}

impl Node {
    /// Creates a new Node instance from an existing [`device::Device`] instance.
    pub fn from(dev: &device::Device) -> Option<ARef<Self>> {
        // SAFETY: By the type invariants, `dev` owns a reference, so it safe to access `ptr`.
        // `fwnode_handle_get` increments the refcount of the `fwnode_handle` if it is not
        // NULL or returns NULL.
        let node = unsafe { bindings::fwnode_handle_get(bindings::dev_fwnode(dev.ptr)) };
        let ptr = ptr::NonNull::new(node)?;
        // SAFETY: `fwnode_handle_get` increments the refcount
        Some(unsafe { ARef::from_raw(ptr.cast()) })
    }
    /// Creates an instance of `NodeIterator`
    ///
    /// This provides an `Iterator` wrapping the internal C functionality of invoking
    /// `fwnode_graph_get_next_endpoint`
    pub fn endpoints(node: ARef<Self>) -> NodeIterator {
        NodeIterator {
            handle: node,
            curr_node: None,
            next_fn: next_endpoint,
        }
    }
    /// Gets the parent to the current `Node` of the device tree
    pub fn parent(node: ARef<Self>) -> Option<ARef<Self>> {
        // SAFETY: By the type invariants, `node` owns a reference, so it safe to the underlying
        // ptr.
        let node = unsafe { bindings::fwnode_get_parent(node.0.get()) };
        let ptr = ptr::NonNull::new(node)?;
        // SAFETY: `fwnode_get_parent` increments the refcount
        Some(unsafe { ARef::from_raw(ptr.cast()) })
    }

    /// Creates an instance of `NodeIterator`
    ///
    /// This provides an `Iterator` wrapping the internal C functionality of invoking
    /// `fwnode_graph_get_next_child_node`
    pub fn children(node: ARef<Self>) -> NodeIterator {
        NodeIterator {
            handle: node,
            curr_node: None,
            next_fn: next_child,
        }
    }
}

type NodeIteratorFn = fn(root: ARef<Node>, curr: &Option<ARef<Node>>) -> Option<ARef<Node>>;

/// Implements the Iterator trait to iterate the device's endpoints given the `Node`
pub struct NodeIterator {
    handle: ARef<Node>,
    curr_node: Option<ARef<Node>>,
    next_fn: NodeIteratorFn,
}
impl Iterator for NodeIterator {
    type Item = ARef<Node>;

    fn next(&mut self) -> Option<Self::Item> {
        self.curr_node = (self.next_fn)(self.handle.clone(), &self.curr_node);
        self.curr_node.clone()
    }
}

fn next_endpoint(node: ARef<Node>, curr: &Option<ARef<Node>>) -> Option<ARef<Node>> {
    let res_ptr = match curr {
        // SAFETY: By the type invariants, `node` has a refcount > 1, so it is safe to access the
        // underlying ptr
        None => unsafe {
            bindings::fwnode_graph_get_next_endpoint(
                node.0.get(),
                ptr::null_mut::<bindings::fwnode_handle>(),
            )
        },
        // SAFETY: By the type invariants, `node` has a refcount > 1, so it is safe to access the
        // underlying ptr. `curr`, by the type invariants has a refcount > 1, hence its safe to access the
        // it's underlying ptr
        Some(curr) => unsafe {
            bindings::fwnode_graph_get_next_endpoint(node.0.get(), curr.0.get())
        },
    };
    let ptr = ptr::NonNull::new(res_ptr)?;
    // SAFETY: `fwnode_graph_get_next_endpoint` increments the refcount before returning
    Some(unsafe { ARef::from_raw(ptr.cast()) })
}

fn next_child(node: ARef<Node>, curr: &Option<ARef<Node>>) -> Option<ARef<Node>> {
    let res_ptr = match curr {
        // SAFETY: By the type invariants, `node` has a refcount > 1, so it is safe to access the
        // underlying ptr
        None => unsafe {
            bindings::fwnode_get_next_child_node(
                node.0.get(),
                ptr::null_mut::<bindings::fwnode_handle>(),
            )
        },
        // SAFETY: By the type invariants, `node` has a refcount > 1, so it is safe to access the
        // underlying ptr. `curr`, by the type invariants has a refcount > 1, hence its safe to access the
        // it's underlying ptr
        Some(curr) => unsafe { bindings::fwnode_get_next_child_node(node.0.get(), curr.0.get()) },
    };
    let ptr = ptr::NonNull::new(res_ptr)?;
    // SAFETY: `fwnode_graph_get_next_child_node` increments the refcount before returning
    Some(unsafe { ARef::from_raw(ptr.cast()) })
}
