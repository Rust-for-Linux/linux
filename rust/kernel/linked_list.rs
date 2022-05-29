// SPDX-License-Identifier: GPL-2.0

//! Linked lists.
//!
//! TODO: This module is a work in progress.

use alloc::boxed::Box;
use core::ptr::NonNull;

pub use crate::raw_list::{Cursor, GetLinks, Links};
use crate::{raw_list, raw_list::RawList, sync::Ref};

// TODO: Use the one from `kernel::file_operations::PointerWrapper` instead.
/// Wraps an object to be inserted in a linked list.
pub trait Wrapper<T: ?Sized> {
    /// Converts the wrapped object into a pointer that represents it.
    fn into_pointer(self) -> NonNull<T>;

    /// Converts the object back from the pointer representation.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous call to [`Wrapper::into_pointer()`].
    unsafe fn from_pointer(ptr: NonNull<T>) -> Self;

    /// Returns a reference to the wrapped object.
    fn as_ref(&self) -> &T;
}

impl<T: ?Sized> Wrapper<T> for Box<T> {
    fn into_pointer(self) -> NonNull<T> {
        NonNull::new(Box::into_raw(self)).unwrap()
    }

    unsafe fn from_pointer(ptr: NonNull<T>) -> Self {
        unsafe { Box::from_raw(ptr.as_ptr()) }
    }

    fn as_ref(&self) -> &T {
        AsRef::as_ref(self)
    }
}

impl<T: ?Sized> Wrapper<T> for Ref<T> {
    fn into_pointer(self) -> NonNull<T> {
        NonNull::new(Ref::into_raw(self) as _).unwrap()
    }

    unsafe fn from_pointer(ptr: NonNull<T>) -> Self {
        // SAFETY: The safety requirements of `from_pointer` satisfy the ones from `Ref::from_raw`.
        unsafe { Ref::from_raw(ptr.as_ptr() as _) }
    }

    fn as_ref(&self) -> &T {
        AsRef::as_ref(self)
    }
}

impl<T: ?Sized> Wrapper<T> for &T {
    fn into_pointer(self) -> NonNull<T> {
        NonNull::from(self)
    }

    unsafe fn from_pointer(ptr: NonNull<T>) -> Self {
        unsafe { &*ptr.as_ptr() }
    }

    fn as_ref(&self) -> &T {
        self
    }
}

/// A descriptor of wrapped list elements.
pub trait GetLinksWrapped: GetLinks {
    /// Specifies which wrapper (e.g., `Box` and `Arc`) wraps the list entries.
    type Wrapped: Wrapper<Self::EntryType>;
}

impl<T: ?Sized> GetLinksWrapped for Box<T>
where
    Box<T>: GetLinks,
{
    type Wrapped = Box<<Box<T> as GetLinks>::EntryType>;
}

impl<T: GetLinks + ?Sized> GetLinks for Box<T> {
    type EntryType = T::EntryType;
    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType> {
        <T as GetLinks>::get_links(data)
    }
}

impl<T: ?Sized> GetLinksWrapped for Ref<T>
where
    Ref<T>: GetLinks,
{
    type Wrapped = Ref<<Ref<T> as GetLinks>::EntryType>;
}

impl<T: GetLinks + ?Sized> GetLinks for Ref<T> {
    type EntryType = T::EntryType;

    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType> {
        <T as GetLinks>::get_links(data)
    }
}

/// A linked list.
///
/// Elements in the list are wrapped and ownership is transferred to the list while the element is
/// in the list.
pub struct List<G: GetLinksWrapped> {
    list: RawList<G>,
}

// SAFETY: Since the elements are owned by the list, the list is `Send` as long as its elements are
// `Send` as well.
unsafe impl<G> Send for List<G>
where
    G: GetLinksWrapped,
    G::EntryType: Send,
{
}

// SAFETY: Since the elements are owned by the list, the list is `Sync` as long as its elements are
// `Sync` as well.
unsafe impl<G> Sync for List<G>
where
    G: GetLinksWrapped,
    G::EntryType: Sync,
{
}

impl<G: GetLinksWrapped> List<G> {
    /// Constructs a new empty linked list.
    pub fn new() -> Self {
        Self {
            list: RawList::new(),
        }
    }

    /// Returns whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Adds the given object to the end (back) of the list.
    ///
    /// It is dropped if it's already on this (or another) list; this can happen for
    /// reference-counted objects, so dropping means decrementing the reference count.
    pub fn push_back(&mut self, data: G::Wrapped) {
        let ptr = data.into_pointer();

        // SAFETY: We took ownership of the entry, so it is safe to insert it.
        if !unsafe { self.list.push_back(ptr.as_ref()) } {
            // If insertion failed, rebuild object so that it can be freed.
            // SAFETY: We just called `into_pointer` above.
            unsafe { G::Wrapped::from_pointer(ptr) };
        }
    }

    /// Inserts the given object after `existing`.
    ///
    /// It is dropped if it's already on this (or another) list; this can happen for
    /// reference-counted objects, so dropping means decrementing the reference count.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `existing` points to a valid entry that is on the list.
    pub unsafe fn insert_after(&mut self, existing: NonNull<G::EntryType>, data: G::Wrapped) {
        let ptr = data.into_pointer();
        let entry = unsafe { &*existing.as_ptr() };
        if unsafe { !self.list.insert_after(entry, ptr.as_ref()) } {
            // If insertion failed, rebuild object so that it can be freed.
            unsafe { G::Wrapped::from_pointer(ptr) };
        }
    }

    /// Removes the given entry.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `data` is either on this list or in no list. It being on another
    /// list leads to memory unsafety.
    pub unsafe fn remove(&mut self, data: &G::Wrapped) -> Option<G::Wrapped> {
        let entry_ref = Wrapper::as_ref(data);
        if unsafe { self.list.remove(entry_ref) } {
            Some(unsafe { G::Wrapped::from_pointer(NonNull::from(entry_ref)) })
        } else {
            None
        }
    }

    /// Removes the element currently at the front of the list and returns it.
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_front(&mut self) -> Option<G::Wrapped> {
        let front = self.list.pop_front()?;
        // SAFETY: Elements on the list were inserted after a call to `into_pointer `.
        Some(unsafe { G::Wrapped::from_pointer(front) })
    }

    /// Returns a cursor starting on the first (front) element of the list.
    pub fn cursor_front(&self) -> Cursor<'_, G> {
        self.list.cursor_front()
    }

    /// Returns a mutable cursor starting on the first (front) element of the list.
    pub fn cursor_front_mut(&mut self) -> CursorMut<'_, G> {
        CursorMut::new(self.list.cursor_front_mut())
    }
}

impl<G: GetLinksWrapped> Default for List<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: GetLinksWrapped> Drop for List<G> {
    fn drop(&mut self) {
        while self.pop_front().is_some() {}
    }
}

/// A list cursor that allows traversing a linked list and inspecting & mutating elements.
pub struct CursorMut<'a, G: GetLinksWrapped> {
    cursor: raw_list::CursorMut<'a, G>,
}

impl<'a, G: GetLinksWrapped> CursorMut<'a, G> {
    fn new(cursor: raw_list::CursorMut<'a, G>) -> Self {
        Self { cursor }
    }

    /// Returns the element the cursor is currently positioned on.
    pub fn current(&mut self) -> Option<&mut G::EntryType> {
        self.cursor.current()
    }

    /// Removes the element the cursor is currently positioned on.
    ///
    /// After removal, it advances the cursor to the next element.
    pub fn remove_current(&mut self) -> Option<G::Wrapped> {
        let ptr = self.cursor.remove_current()?;

        // SAFETY: Elements on the list were inserted after a call to `into_pointer `.
        Some(unsafe { G::Wrapped::from_pointer(ptr) })
    }

    /// Returns the element immediately after the one the cursor is positioned on.
    pub fn peek_next(&mut self) -> Option<&mut G::EntryType> {
        self.cursor.peek_next()
    }

    /// Returns the element immediately before the one the cursor is positioned on.
    pub fn peek_prev(&mut self) -> Option<&mut G::EntryType> {
        self.cursor.peek_prev()
    }

    /// Moves the cursor to the next element.
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }
}
