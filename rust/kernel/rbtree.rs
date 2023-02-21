// SPDX-License-Identifier: GPL-2.0

//! Red-black trees.
//!
//! C header: [`include/linux/rbtree.h`](../../../../include/linux/rbtree.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/core-api/rbtree.html>

use crate::{bindings, Result};
use alloc::boxed::Box;
use core::{
    cmp::{Ord, Ordering},
    iter::{IntoIterator, Iterator},
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{addr_of_mut, NonNull},
};

struct Node<K, V> {
    links: bindings::rb_node,
    key: K,
    value: V,
}

/// A red-black tree with owned nodes.
///
/// It is backed by the kernel C red-black trees.
///
/// # Invariants
///
/// Non-null parent/children pointers stored in instances of the `rb_node` C struct are always
/// valid, and pointing to a field of our internal representation of a node.
///
/// # Examples
///
/// In the example below we do several operations on a tree. We note that insertions may fail if
/// the system is out of memory.
///
/// ```
/// use kernel::rbtree::RBTree;
///
/// // Create a new tree.
/// let mut tree = RBTree::new();
///
/// // Insert three elements.
/// tree.try_insert(20, 200)?;
/// tree.try_insert(10, 100)?;
/// tree.try_insert(30, 300)?;
///
/// // Check the nodes we just inserted.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &100));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert_eq!(iter.next().unwrap(), (&30, &300));
///     assert!(iter.next().is_none());
/// }
///
/// // Print all elements.
/// for (key, value) in &tree {
///     pr_info!("{} = {}\n", key, value);
/// }
///
/// // Replace one of the elements.
/// tree.try_insert(10, 1000)?;
///
/// // Check that the tree reflects the replacement.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &1000));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert_eq!(iter.next().unwrap(), (&30, &300));
///     assert!(iter.next().is_none());
/// }
///
/// // Change the value of one of the elements.
/// *tree.get_mut(&30).unwrap() = 3000;
///
/// // Check that the tree reflects the update.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &1000));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert_eq!(iter.next().unwrap(), (&30, &3000));
///     assert!(iter.next().is_none());
/// }
///
/// // Remove an element.
/// tree.remove(&10);
///
/// // Check that the tree reflects the removal.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert_eq!(iter.next().unwrap(), (&30, &3000));
///     assert!(iter.next().is_none());
/// }
///
/// // Update all values.
/// for value in tree.values_mut() {
///     *value *= 10;
/// }
///
/// // Check that the tree reflects the changes to values.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&20, &2000));
///     assert_eq!(iter.next().unwrap(), (&30, &30000));
///     assert!(iter.next().is_none());
/// }
///
/// # Ok::<(), Error>(())
/// ```
///
/// In the example below, we first allocate a node, acquire a spinlock, then insert the node into
/// the tree. This is useful when the insertion context does not allow sleeping, for example, when
/// holding a spinlock.
///
/// ```
/// use kernel::{rbtree::RBTree, sync::SpinLock};
///
/// fn insert_test(tree: &SpinLock<RBTree<u32, u32>>) -> Result {
///     // Pre-allocate node. This may fail (as it allocates memory).
///     let node = RBTree::try_allocate_node(10, 100)?;
///
///     // Insert node while holding the lock. It is guaranteed to succeed with no allocation
///     // attempts.
///     let mut guard = tree.lock();
///     guard.insert(node);
///     Ok(())
/// }
/// ```
///
/// In the example below, we reuse an existing node allocation from an element we removed.
///
/// ```
/// use kernel::rbtree::RBTree;
///
/// // Create a new tree.
/// let mut tree = RBTree::new();
///
/// // Insert three elements.
/// tree.try_insert(20, 200)?;
/// tree.try_insert(10, 100)?;
/// tree.try_insert(30, 300)?;
///
/// // Check the nodes we just inserted.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &100));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert_eq!(iter.next().unwrap(), (&30, &300));
///     assert!(iter.next().is_none());
/// }
///
/// // Remove a node, getting back ownership of it.
/// let existing = tree.remove_node(&30).unwrap();
///
/// // Check that the tree reflects the removal.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &100));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert!(iter.next().is_none());
/// }
///
/// // Turn the node into a reservation so that we can reuse it with a different key/value.
/// let reservation = existing.into_reservation();
///
/// // Insert a new node into the tree, reusing the previous allocation. This is guaranteed to
/// // succeed (no memory allocations).
/// tree.insert(reservation.into_node(15, 150));
///
/// // Check that the tree reflect the new insertion.
/// {
///     let mut iter = tree.iter();
///     assert_eq!(iter.next().unwrap(), (&10, &100));
///     assert_eq!(iter.next().unwrap(), (&15, &150));
///     assert_eq!(iter.next().unwrap(), (&20, &200));
///     assert!(iter.next().is_none());
/// }
///
/// # Ok::<(), Error>(())
/// ```
pub struct RBTree<K, V> {
    root: bindings::rb_root,
    _p: PhantomData<Node<K, V>>,
}

impl<K, V> RBTree<K, V> {
    /// Creates a new and empty tree.
    pub fn new() -> Self {
        Self {
            // INVARIANT: There are no nodes in the tree, so the invariant holds vacuously.
            root: bindings::rb_root::default(),
            _p: PhantomData,
        }
    }

    /// Tries to insert a new value into the tree.
    ///
    /// It overwrites a node if one already exists with the same key and returns it (containing the
    /// key/value pair). Returns [`None`] if a node with the same key didn't already exist.
    ///
    /// Returns an error if it cannot allocate memory for the new node.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<Option<RBTreeNode<K, V>>>
    where
        K: Ord,
    {
        Ok(self.insert(Self::try_allocate_node(key, value)?))
    }

    /// Allocates memory for a node to be eventually initialised and inserted into the tree via a
    /// call to [`RBTree::insert`].
    pub fn try_reserve_node() -> Result<RBTreeNodeReservation<K, V>> {
        Ok(RBTreeNodeReservation {
            node: Box::try_new(MaybeUninit::uninit())?,
        })
    }

    /// Allocates and initialiases a node that can be inserted into the tree via
    /// [`RBTree::insert`].
    pub fn try_allocate_node(key: K, value: V) -> Result<RBTreeNode<K, V>> {
        Ok(Self::try_reserve_node()?.into_node(key, value))
    }

    /// Inserts a new node into the tree.
    ///
    /// It overwrites a node if one already exists with the same key and returns it (containing the
    /// key/value pair). Returns [`None`] if a node with the same key didn't already exist.
    ///
    /// This function always succeeds.
    pub fn insert(&mut self, node: RBTreeNode<K, V>) -> Option<RBTreeNode<K, V>>
    where
        K: Ord,
    {
        let RBTreeNode { node } = node;
        let node = Box::into_raw(node);
        // SAFETY: `node` is valid at least until we call `Box::from_raw`, which only happens when
        // the node is removed or replaced.
        let node_links = unsafe { addr_of_mut!((*node).links) };
        let mut new_link: &mut *mut bindings::rb_node = &mut self.root.rb_node;
        let mut parent = core::ptr::null_mut();
        while !new_link.is_null() {
            let this = crate::container_of!(*new_link, Node<K, V>, links);

            parent = *new_link;

            // SAFETY: `this` is a non-null node so it is valid by the type invariants. `node` is
            // valid until the node is removed.
            match unsafe { (*node).key.cmp(&(*this).key) } {
                // SAFETY: `parent` is a non-null node so it is valid by the type invariants.
                Ordering::Less => new_link = unsafe { &mut (*parent).rb_left },
                // SAFETY: `parent` is a non-null node so it is valid by the type invariants.
                Ordering::Greater => new_link = unsafe { &mut (*parent).rb_right },
                Ordering::Equal => {
                    // INVARIANT: We are replacing an existing node with a new one, which is valid.
                    // It remains valid because we "forgot" it with `Box::into_raw`.
                    // SAFETY: All pointers are non-null and valid (parent, despite the name, really
                    // is the node we're replacing).
                    unsafe { bindings::rb_replace_node(parent, node_links, &mut self.root) };

                    // INVARIANT: The node is being returned and the caller may free it, however,
                    // it was removed from the tree. So the invariants still hold.
                    return Some(RBTreeNode {
                        // SAFETY: `this` was a node in the tree, so it is valid.
                        node: unsafe { Box::from_raw(this as _) },
                    });
                }
            }
        }

        // INVARIANT: We are linking in a new node, which is valid. It remains valid because we
        // "forgot" it with `Box::into_raw`.
        // SAFETY: All pointers are non-null and valid (`*new_link` is null, but `new_link` is a
        // mutable reference).
        unsafe { bindings::rb_link_node(node_links, parent, new_link) };

        // SAFETY: All pointers are valid. `node` has just been inserted into the tree.
        unsafe { bindings::rb_insert_color(node_links, &mut self.root) };
        None
    }

    /// Returns a node with the given key, if one exists.
    fn find(&self, key: &K) -> Option<NonNull<Node<K, V>>>
    where
        K: Ord,
    {
        let mut node = self.root.rb_node;
        while !node.is_null() {
            let this = crate::container_of!(node, Node<K, V>, links);
            // SAFETY: `this` is a non-null node so it is valid by the type invariants.
            node = match key.cmp(unsafe { &(*this).key }) {
                // SAFETY: `node` is a non-null node so it is valid by the type invariants.
                Ordering::Less => unsafe { (*node).rb_left },
                // SAFETY: `node` is a non-null node so it is valid by the type invariants.
                Ordering::Greater => unsafe { (*node).rb_right },
                Ordering::Equal => return NonNull::new(this as _),
            }
        }
        None
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: Ord,
    {
        // SAFETY: The `find` return value is a node in the tree, so it is valid.
        self.find(key).map(|node| unsafe { &node.as_ref().value })
    }

    /// Returns a mutable reference to the value corresponding to the key.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V>
    where
        K: Ord,
    {
        // SAFETY: The `find` return value is a node in the tree, so it is valid.
        self.find(key)
            .map(|mut node| unsafe { &mut node.as_mut().value })
    }

    /// Removes the node with the given key from the tree.
    ///
    /// It returns the node that was removed if one exists, or [`None`] otherwise.
    pub fn remove_node(&mut self, key: &K) -> Option<RBTreeNode<K, V>>
    where
        K: Ord,
    {
        let mut node = self.find(key)?;

        // SAFETY: The `find` return value is a node in the tree, so it is valid.
        unsafe { bindings::rb_erase(&mut node.as_mut().links, &mut self.root) };

        // INVARIANT: The node is being returned and the caller may free it, however, it was
        // removed from the tree. So the invariants still hold.
        Some(RBTreeNode {
            // SAFETY: The `find` return value was a node in the tree, so it is valid.
            node: unsafe { Box::from_raw(node.as_ptr()) },
        })
    }

    /// Removes the node with the given key from the tree.
    ///
    /// It returns the value that was removed if one exists, or [`None`] otherwise.
    pub fn remove(&mut self, key: &K) -> Option<V>
    where
        K: Ord,
    {
        let node = self.remove_node(key)?;
        let RBTreeNode { node } = node;
        let Node {
            links: _,
            key: _,
            value,
        } = *node;
        Some(value)
    }

    /// Returns an iterator over the tree nodes, sorted by key.
    pub fn iter(&self) -> RBTreeIterator<'_, K, V> {
        RBTreeIterator {
            _tree: PhantomData,
            // SAFETY: `root` is valid as it's embedded in `self` and we have a valid `self`.
            next: unsafe { bindings::rb_first(&self.root) },
        }
    }

    /// Returns a cursor over the tree nodes, starting with the smallest key
    /// ```
    /// use kernel::rbtree::RBTree;
    ///
    /// // Create a new tree.
    /// let mut tree = RBTree::new();
    ///
    /// // Insert three elements.
    /// tree.try_insert(20, 200)?;
    /// tree.try_insert(10, 100)?;
    /// tree.try_insert(30, 300)?;
    ///
    /// let cursor = tree.cursor_front();
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&10, &100)));
    ///
    /// # Ok::<(), Error>(())
    pub fn cursor_front(&mut self) -> RBTreeCursor<'_, K, V> {
        RBTreeCursor {
            _tree: PhantomData,
            root: self.root,
            current: unsafe { bindings::rb_first(&self.root) },
        }
    }

    /// Returns a cursor over the tree nodes, starting with the smallest key
    /// ```
    /// use kernel::rbtree::RBTree;
    ///
    /// // Create a new tree.
    /// let mut tree = RBTree::new();
    ///
    /// // Insert three elements.
    /// tree.try_insert(20, 200)?;
    /// tree.try_insert(10, 100)?;
    /// tree.try_insert(30, 300)?;
    ///
    /// let cursor = tree.cursor_back();
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&30, &300)));
    ///
    /// # Ok::<(), Error>(())
    pub fn cursor_back(&mut self) -> RBTreeCursor<'_, K, V> {
        RBTreeCursor {
            _tree: PhantomData,
            root: self.root,
            current: unsafe { bindings::rb_last(&self.root) },
        }
    }

    /// Returns a cursor over the tree nodes, starting the node matching the given `search_key`,
    /// else the nearest node with a key smaller/larger than `search_key`, based on the given `direction`.
    /// ```
    /// use kernel::rbtree::RBTree;
    /// use kernel::rbtree::Direction;
    ///
    /// // Create a new tree.
    /// let mut tree = RBTree::new();
    ///
    /// // Insert three elements.
    /// tree.try_insert(20, 200)?;
    /// tree.try_insert(10, 100)?;
    /// tree.try_insert(30, 300)?;
    ///
    /// let cursor = tree.cursor_search(&20, Direction::Next);
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&20, &200)));
    /// 
    /// let cursor = tree.cursor_search(&25, Direction::Next);
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&30, &300)));
    /// 
    /// let cursor = tree.cursor_search(&15, Direction::Prev);
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&10, &100)));
    /// 
    /// // TODO: fix errors when searching less than smallest etc.!
    ///
    /// # Ok::<(), Error>(())
    pub fn cursor_search(&mut self, search_key: &K, direction: Direction) -> RBTreeCursor<'_, K, V>
    where
        K: Ord,
    {
        let mut node = self.root.rb_node;
        let mut best_match: Option<NonNull<Node<K, V>>> = None;
        while !node.is_null() {
            let this = crate::container_of!(node, Node<K, V>, links) as *mut Node<K, V>;
            // SAFETY: `this` is a non-null node so it is valid by the type invariants.
            let this_key = unsafe { &(*this).key };
            // SAFETY: `node` is a non-null node so it is valid by the type invariants.
            let left_child = unsafe { (*node).rb_left };
            // SAFETY: `node` is a non-null node so it is valid by the type invariants.
            let right_child = unsafe { (*node).rb_right };
            if search_key == this_key {
                return RBTreeCursor {
                    _tree: PhantomData,
                    root: self.root,
                    current: node,
                };
            } else {
                node = match direction {
                    Direction::Prev => {
                        if search_key < this_key {
                            left_child
                        } else {
                            let is_better_match = match best_match {
                                None => true,
                                Some(best) => {
                                    // SAFETY: `best` is a non-null node so it is valid by the type invariants.
                                    let best_key = unsafe { &(*best.as_ptr()).key };
                                    best_key < this_key
                                }
                            };
                            if is_better_match {
                                best_match = NonNull::new(this);
                            }
                            right_child
                        }
                    }
                    Direction::Next => {
                        if search_key > this_key {
                            right_child
                        } else {
                            let is_better_match = match best_match {
                                None => true,
                                Some(best) => {
                                    // SAFETY: `best` is a non-null node so it is valid by the type invariants.
                                    let best_key = unsafe { &(*best.as_ptr()).key };
                                    best_key > this_key
                                }
                            };
                            if is_better_match {
                                best_match = NonNull::new(this);
                            }
                            left_child
                        }
                    }
                }
            }
        }
        RBTreeCursor {
            _tree: PhantomData,
            root: self.root,
            current: match best_match {
                None => self.root.rb_node,
                Some(best) => {
                    // SAFETY: `best` is a non-null node so it is valid by the type invariants.
                    unsafe { addr_of_mut!((*best.as_ptr()).links) }
                }
            },
        }
    }

    /// Returns a mutable iterator over the tree nodes, sorted by key.
    pub fn iter_mut(&mut self) -> RBTreeIteratorMut<'_, K, V> {
        RBTreeIteratorMut {
            _tree: PhantomData,
            // SAFETY: `root` is valid as it's embedded in `self` and we have a valid `self`.
            next: unsafe { bindings::rb_first(&self.root) },
        }
    }

    /// Returns an iterator over the keys of the nodes in the tree, in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &'_ K> {
        self.iter().map(|(k, _)| k)
    }

    /// Returns an iterator over the values of the nodes in the tree, sorted by key.
    pub fn values(&self) -> impl Iterator<Item = &'_ V> {
        self.iter().map(|(_, v)| v)
    }

    /// Returns a mutable iterator over the values of the nodes in the tree, sorted by key.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &'_ mut V> {
        self.iter_mut().map(|(_, v)| v)
    }
}

impl<K, V> Default for RBTree<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Drop for RBTree<K, V> {
    fn drop(&mut self) {
        // SAFETY: `root` is valid as it's embedded in `self` and we have a valid `self`.
        let mut next = unsafe { bindings::rb_first_postorder(&self.root) };

        // INVARIANT: The loop invariant is that all tree nodes from `next` in postorder are valid.
        while !next.is_null() {
            let this = crate::container_of!(next, Node<K, V>, links);

            // Find out what the next node is before disposing of the current one.
            // SAFETY: `next` and all nodes in postorder are still valid.
            next = unsafe { bindings::rb_next_postorder(next) };

            // INVARIANT: This is the destructor, so we break the type invariant during clean-up,
            // but it is not observable. The loop invariant is still maintained.
            // SAFETY: `this` is valid per the loop invariant.
            unsafe { Box::from_raw(this as *mut Node<K, V>) };
        }
    }
}

/// Direction for `RBTreeCursor` operations
pub enum Direction {
    /// the node immediately before, in sort order
    Prev,
    /// the node immediately after, in sort order
    Next,
}

/// A bidirectional cursor over the tree nodes, sorted by key
pub struct RBTreeCursor<'a, K, V> {
    _tree: PhantomData<&'a RBTree<K, V>>,
    root: bindings::rb_root,
    current: *mut bindings::rb_node,
}

impl<'a, K, V> RBTreeCursor<'a, K, V> {
    /// The current node
    pub fn current(&self) -> Option<(&K, &V)> {
        self.to_key_value(self.current)
    }

    /// The current node, with a mutable value
    /// ```
    /// use kernel::rbtree::RBTree;
    ///
    /// // Create a new tree.
    /// let mut tree = RBTree::new();
    ///
    /// // Insert three elements.
    /// tree.try_insert(20, 200)?;
    /// tree.try_insert(10, 100)?;
    /// tree.try_insert(30, 300)?;
    ///
    /// let mut cursor = tree.cursor_front();
    /// let (k, v) = cursor.current_mut().unwrap();
    /// *v = 1000;
    /// let updated = tree.get(&10).unwrap();
    /// assert_eq!(updated, &1000);
    ///
    /// # Ok::<(), Error>(())
    pub fn current_mut(&mut self) -> Option<(&K, &mut V)> {
        unsafe { self.to_key_value_mut(self.current) }
    }

    /// Removes and returns the current node, setting the new current
    /// node based on the given `direction`.  
    /// The new current node is not guaranteed to be the prev/next node.
    /// If there is no such prev/next node, but the complement exists,
    /// it will be set regardless of `direction`.
    /// ```
    /// use kernel::rbtree::RBTree;
    /// use kernel::rbtree::Direction;
    ///
    /// // Create a new tree.
    /// let mut tree = RBTree::new();
    ///
    /// // Insert three elements.
    /// tree.try_insert(20, 200)?;
    /// tree.try_insert(10, 100)?;
    /// tree.try_insert(30, 300)?;
    ///
    /// let mut cursor = tree.cursor_front();
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&10, &100)));
    /// cursor.remove_current(Direction::Next);
    /// let current = cursor.current();
    /// assert_eq!(current, Some((&20, &200)));
    ///
    /// # Ok::<(), Error>(())
    pub fn remove_current(&mut self, direction: Direction) -> Option<(&K, &V)> {
        if self.current.is_null() {
            return None;
        }

        // SAFETY: The reference to the tree used to create the cursor outlives the cursor, so
        // the tree cannot change. By the tree invariant, all nodes are valid.
        let prev = unsafe { self.get_neighbor_raw(Direction::Prev) };
        // SAFETY: The reference to the tree used to create the cursor outlives the cursor, so
        // the tree cannot change. By the tree invariant, all nodes are valid.
        let next = unsafe { self.get_neighbor_raw(Direction::Next) };

        let this = crate::container_of!(self.current, Node<K, V>, links) as *mut Node<K, V>;
        // SAFETY: The reference to the tree used to create the cursor outlives the cursor, so
        // the tree cannot change. By the tree invariant, all nodes are valid.
        unsafe { bindings::rb_erase(&mut (*this).links, &mut self.root) };

        let new_current = match (direction, prev, next) {
            (_, None, None) => None,
            (Direction::Prev, Some(prev), _) => Some(prev),
            (Direction::Next, _, Some(next)) => Some(next),
            (Direction::Prev, None, Some(next)) => Some(next),
            (Direction::Next, Some(prev), None) => Some(prev),
        };

        let removed = self.to_key_value(self.current);
        self.current = match new_current {
            Some(n) => n,
            None => core::ptr::null_mut(),
        };
        removed
    }

    /// Move the cursor to the previous node, returning its key/value
    pub fn prev(&mut self) -> Option<(&K, &V)> {
        // SAFETY: `get_neighbor_raw` returns a non-null pointer inside the Option, we have the only mutable reference to self
        let prev = unsafe { self.get_neighbor_raw(Direction::Prev) };
        match prev {
            None => {
                self.current = core::ptr::null_mut();
                None
            }
            Some(n) => {
                self.current = n;
                self.to_key_value(n)
            }
        }
    }

    /// Move the cursor to the previous node, returning its key and a mutable reference to its value
    pub fn prev_mut(&mut self) -> Option<(&K, &mut V)> {
        // SAFETY: `get_neighbor_raw` returns a non-null pointer inside the Option, we have the only mutable reference to self
        let prev = unsafe { self.get_neighbor_raw(Direction::Prev) };
        match prev {
            None => {
                self.current = core::ptr::null_mut();
                None
            }
            Some(n) => {
                self.current = n;
                // SAFETY: we have hte only  mutable reference to self
                unsafe { self.to_key_value_mut(n) }
            }
        }
    }

    /// View the previous node without moving the cursor
    pub fn peek_prev(&self) -> Option<(&K, &V)> {
        self.get_neighbor(Direction::Prev)
    }

    fn get_neighbor(&self, direction: Direction) -> Option<(&K, &V)> {
        if self.current.is_null() {
            return None;
        }

        // SAFETY: `self.current` is a non-null node so it is valid by the type invariants.
        let neighbor = unsafe {
            match direction {
                Direction::Prev => bindings::rb_prev(self.current),
                Direction::Next => bindings::rb_next(self.current),
            }
        };

        self.to_key_value(neighbor)
    }

    unsafe fn get_neighbor_raw(&mut self, direction: Direction) -> Option<*mut bindings::rb_node> {
        if self.current.is_null() {
            return None;
        }

        // SAFETY: `self.current` is a non-null node so it is valid by the type invariants.
        let neighbor = unsafe {
            match direction {
                Direction::Prev => bindings::rb_prev(self.current),
                Direction::Next => bindings::rb_next(self.current),
            }
        };

        if neighbor.is_null() {
            return None;
        }

        return Some(neighbor);
    }

    fn to_key_value(&self, node: *mut bindings::rb_node) -> Option<(&'a K, &'a V)> {
        if node.is_null() {
            return None;
        }
        let this = crate::container_of!(node, Node<K, V>, links) as *mut Node<K, V>;
        // SAFETY: `this` is a non-null node so it is valid by the type invariants.
        let k = unsafe { &(*this).key };
        // SAFETY: `this` is a non-null node so it is valid by the type invariants.
        let v = unsafe { &(*this).value };
        Some((k, v))
    }

    unsafe fn to_key_value_mut(
        &mut self,
        node: *mut bindings::rb_node,
    ) -> Option<(&'a K, &'a mut V)> {
        if node.is_null() {
            return None;
        }
        let this = crate::container_of!(node, Node<K, V>, links) as *mut Node<K, V>;
        // SAFETY: `this` is a non-null node so it is valid by the type invariants.
        let k = unsafe { &(*this).key };
        // SAFETY: `this` is a non-null node so it is valid by the type invariants.
        let v = unsafe { &mut (*this).value };
        Some((k, v))
    }
}

impl<'a, K, V> IntoIterator for &'a RBTree<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = RBTreeIterator<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over the nodes of a [`RBTree`].
///
/// Instances are created by calling [`RBTree::iter`].
pub struct RBTreeIterator<'a, K, V> {
    _tree: PhantomData<&'a RBTree<K, V>>,
    next: *mut bindings::rb_node,
}

impl<'a, K, V> Iterator for RBTreeIterator<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next.is_null() {
            return None;
        }

        let cur = crate::container_of!(self.next, Node<K, V>, links);

        // SAFETY: The reference to the tree used to create the iterator outlives the iterator, so
        // the tree cannot change. By the tree invariant, all nodes are valid.
        self.next = unsafe { bindings::rb_next(self.next) };

        // SAFETY: By the same reasoning above, it is safe to dereference the node. Additionally,
        // it is ok to return a reference to members because the iterator must outlive it.
        Some(unsafe { (&(*cur).key, &(*cur).value) })
    }
}

impl<'a, K, V> IntoIterator for &'a mut RBTree<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = RBTreeIteratorMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// A mutable iterator over the nodes of a [`RBTree`].
///
/// Instances are created by calling [`RBTree::iter_mut`].
pub struct RBTreeIteratorMut<'a, K, V> {
    _tree: PhantomData<&'a RBTree<K, V>>,
    next: *mut bindings::rb_node,
}

impl<'a, K, V> Iterator for RBTreeIteratorMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next.is_null() {
            return None;
        }

        let cur = crate::container_of!(self.next, Node<K, V>, links) as *mut Node<K, V>;

        // SAFETY: The reference to the tree used to create the iterator outlives the iterator, so
        // the tree cannot change (except for the value of previous nodes, but those don't affect
        // the iteration process). By the tree invariant, all nodes are valid.
        self.next = unsafe { bindings::rb_next(self.next) };

        // SAFETY: By the same reasoning above, it is safe to dereference the node. Additionally,
        // it is ok to return a reference to members because the iterator must outlive it.
        Some(unsafe { (&(*cur).key, &mut (*cur).value) })
    }
}

/// A memory reservation for a red-black tree node.
///
/// It contains the memory needed to hold a node that can be inserted into a red-black tree. One
/// can be obtained by directly allocating it ([`RBTree::try_reserve_node`]) or by "uninitialising"
/// ([`RBTreeNode::into_reservation`]) an actual node (usually returned by some operation like
/// removal from a tree).
pub struct RBTreeNodeReservation<K, V> {
    node: Box<MaybeUninit<Node<K, V>>>,
}

impl<K, V> RBTreeNodeReservation<K, V> {
    /// Initialises a node reservation.
    ///
    /// It then becomes an [`RBTreeNode`] that can be inserted into a tree.
    pub fn into_node(mut self, key: K, value: V) -> RBTreeNode<K, V> {
        let node_ptr = self.node.as_mut_ptr();
        // SAFETY: `node_ptr` is valid, and so are its fields.
        unsafe { addr_of_mut!((*node_ptr).links).write(bindings::rb_node::default()) };
        // SAFETY: `node_ptr` is valid, and so are its fields.
        unsafe { addr_of_mut!((*node_ptr).key).write(key) };
        // SAFETY: `node_ptr` is valid, and so are its fields.
        unsafe { addr_of_mut!((*node_ptr).value).write(value) };
        let raw = Box::into_raw(self.node);
        RBTreeNode {
            // SAFETY: The pointer came from a `MaybeUninit<Node>` whose fields have all been
            // initialised. Additionally, it has the same layout as `Node`.
            node: unsafe { Box::from_raw(raw as _) },
        }
    }
}

/// A red-black tree node.
///
/// The node is fully initialised (with key and value) and can be inserted into a tree without any
/// extra allocations or failure paths.
pub struct RBTreeNode<K, V> {
    node: Box<Node<K, V>>,
}

impl<K, V> RBTreeNode<K, V> {
    /// "Uninitialises" a node.
    ///
    /// It then becomes a reservation that can be re-initialised into a different node (i.e., with
    /// a different key and/or value).
    ///
    /// The existing key and value are dropped in-place as part of this operation, that is, memory
    /// may be freed (but only for the key/value; memory for the node itself is kept for reuse).
    pub fn into_reservation(self) -> RBTreeNodeReservation<K, V> {
        let raw = Box::into_raw(self.node);
        let mut ret = RBTreeNodeReservation {
            // SAFETY: The pointer came from a valid `Node`, which has the same layout as
            // `MaybeUninit<Node>`.
            node: unsafe { Box::from_raw(raw as _) },
        };
        // SAFETY: Although the type is `MaybeUninit<Node>`, we know it has been initialised
        // because it came from a `Node`. So it is safe to drop it.
        unsafe { core::ptr::drop_in_place(ret.node.as_mut_ptr()) };
        ret
    }
}
