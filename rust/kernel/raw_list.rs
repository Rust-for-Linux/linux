// SPDX-License-Identifier: GPL-2.0

//! Raw lists.
//!
//! TODO: This module is a work in progress.

use core::{
    cell::UnsafeCell,
    ptr,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

/// A descriptor of list elements.
///
/// It describes the type of list elements and provides a function to determine how to get the
/// links to be used on a list.
///
/// A type that may be in multiple lists simultaneously needs to implement one of these for each
/// simultaneous list.
pub trait GetLinks {
    /// The type of the entries in the list.
    type EntryType: ?Sized;

    /// Returns the links to be used when linking an entry within a list.
    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType>;
}

/// The links used to link an object on a linked list.
///
/// Instances of this type are usually embedded in structures and returned in calls to
/// [`GetLinks::get_links`].
pub struct Links<T: ?Sized> {
    inserted: AtomicBool,
    entry: UnsafeCell<ListEntry<T>>,
}

impl<T: ?Sized> Links<T> {
    /// Constructs a new [`Links`] instance that isn't inserted on any lists yet.
    pub fn new() -> Self {
        Self {
            inserted: AtomicBool::new(false),
            entry: UnsafeCell::new(ListEntry::new()),
        }
    }

    fn acquire_for_insertion(&self) -> bool {
        self.inserted
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    fn release_after_removal(&self) {
        self.inserted.store(false, Ordering::Release);
    }
}

impl<T: ?Sized> Default for Links<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct ListEntry<T: ?Sized> {
    next: Option<NonNull<T>>,
    prev: Option<NonNull<T>>,
}

impl<T: ?Sized> ListEntry<T> {
    fn new() -> Self {
        Self {
            next: None,
            prev: None,
        }
    }
}

/// A linked list.
///
/// # Invariants
///
/// The links of objects added to a list are owned by the list.
pub(crate) struct RawList<G: GetLinks> {
    head: Option<NonNull<G::EntryType>>,
}

impl<G: GetLinks> RawList<G> {
    pub(crate) fn new() -> Self {
        Self { head: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    fn insert_after_priv(
        &mut self,
        existing: &G::EntryType,
        new_entry: &mut ListEntry<G::EntryType>,
        new_ptr: Option<NonNull<G::EntryType>>,
    ) {
        {
            // SAFETY: It's safe to get the previous entry of `existing` because the list cannot
            // change.
            let existing_links = unsafe { &mut *G::get_links(existing).entry.get() };
            new_entry.next = existing_links.next;
            existing_links.next = new_ptr;
        }

        new_entry.prev = Some(NonNull::from(existing));

        // SAFETY: It's safe to get the next entry of `existing` because the list cannot change.
        let next_links =
            unsafe { &mut *G::get_links(new_entry.next.unwrap().as_ref()).entry.get() };
        next_links.prev = new_ptr;
    }

    /// Inserts the given object after `existing`.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `existing` points to a valid entry that is on the list.
    pub(crate) unsafe fn insert_after(
        &mut self,
        existing: &G::EntryType,
        new: &G::EntryType,
    ) -> bool {
        let links = G::get_links(new);
        if !links.acquire_for_insertion() {
            // Nothing to do if already inserted.
            return false;
        }

        // SAFETY: The links are now owned by the list, so it is safe to get a mutable reference.
        let new_entry = unsafe { &mut *links.entry.get() };
        self.insert_after_priv(existing, new_entry, Some(NonNull::from(new)));
        true
    }

    fn push_back_internal(&mut self, new: &G::EntryType) -> bool {
        let links = G::get_links(new);
        if !links.acquire_for_insertion() {
            // Nothing to do if already inserted.
            return false;
        }

        // SAFETY: The links are now owned by the list, so it is safe to get a mutable reference.
        let new_entry = unsafe { &mut *links.entry.get() };
        let new_ptr = Some(NonNull::from(new));
        match self.back() {
            // SAFETY: `back` is valid as the list cannot change.
            Some(back) => self.insert_after_priv(unsafe { back.as_ref() }, new_entry, new_ptr),
            None => {
                self.head = new_ptr;
                new_entry.next = new_ptr;
                new_entry.prev = new_ptr;
            }
        }
        true
    }

    pub(crate) unsafe fn push_back(&mut self, new: &G::EntryType) -> bool {
        self.push_back_internal(new)
    }

    fn remove_internal(&mut self, data: &G::EntryType) -> bool {
        let links = G::get_links(data);

        // SAFETY: The links are now owned by the list, so it is safe to get a mutable reference.
        let entry = unsafe { &mut *links.entry.get() };
        let next = if let Some(next) = entry.next {
            next
        } else {
            // Nothing to do if the entry is not on the list.
            return false;
        };

        if ptr::eq(data, next.as_ptr()) {
            // We're removing the only element.
            self.head = None
        } else {
            // Update the head if we're removing it.
            if let Some(raw_head) = self.head {
                if ptr::eq(data, raw_head.as_ptr()) {
                    self.head = Some(next);
                }
            }

            // SAFETY: It's safe to get the previous entry because the list cannot change.
            unsafe { &mut *G::get_links(entry.prev.unwrap().as_ref()).entry.get() }.next =
                entry.next;

            // SAFETY: It's safe to get the next entry because the list cannot change.
            unsafe { &mut *G::get_links(next.as_ref()).entry.get() }.prev = entry.prev;
        }

        // Reset the links of the element we're removing so that we know it's not on any list.
        entry.next = None;
        entry.prev = None;
        links.release_after_removal();
        true
    }

    /// Removes the given entry.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `data` is either on this list or in no list. It being on another
    /// list leads to memory unsafety.
    pub(crate) unsafe fn remove(&mut self, data: &G::EntryType) -> bool {
        self.remove_internal(data)
    }

    fn pop_front_internal(&mut self) -> Option<NonNull<G::EntryType>> {
        let head = self.head?;
        // SAFETY: The head is on the list as we just got it from there and it cannot change.
        unsafe { self.remove(head.as_ref()) };
        Some(head)
    }

    pub(crate) fn pop_front(&mut self) -> Option<NonNull<G::EntryType>> {
        self.pop_front_internal()
    }

    pub(crate) fn front(&self) -> Option<NonNull<G::EntryType>> {
        self.head
    }

    pub(crate) fn back(&self) -> Option<NonNull<G::EntryType>> {
        // SAFETY: The links of head are owned by the list, so it is safe to get a reference.
        unsafe { &*G::get_links(self.head?.as_ref()).entry.get() }.prev
    }

    pub(crate) fn cursor_front(&self) -> Cursor<'_, G> {
        Cursor::new(self, self.front())
    }

    pub(crate) fn cursor_front_mut(&mut self) -> CursorMut<'_, G> {
        CursorMut::new(self, self.front())
    }
}

struct CommonCursor<G: GetLinks> {
    cur: Option<NonNull<G::EntryType>>,
}

impl<G: GetLinks> CommonCursor<G> {
    fn new(cur: Option<NonNull<G::EntryType>>) -> Self {
        Self { cur }
    }

    fn move_next(&mut self, list: &RawList<G>) {
        match self.cur.take() {
            None => self.cur = list.head,
            Some(cur) => {
                if let Some(head) = list.head {
                    // SAFETY: We have a shared ref to the linked list, so the links can't change.
                    let links = unsafe { &*G::get_links(cur.as_ref()).entry.get() };
                    if links.next.unwrap() != head {
                        self.cur = links.next;
                    }
                }
            }
        }
    }

    fn move_prev(&mut self, list: &RawList<G>) {
        match list.head {
            None => self.cur = None,
            Some(head) => {
                let next = match self.cur.take() {
                    None => head,
                    Some(cur) => {
                        if cur == head {
                            return;
                        }
                        cur
                    }
                };
                // SAFETY: There's a shared ref to the list, so the links can't change.
                let links = unsafe { &*G::get_links(next.as_ref()).entry.get() };
                self.cur = links.prev;
            }
        }
    }
}

/// A list cursor that allows traversing a linked list and inspecting elements.
pub struct Cursor<'a, G: GetLinks> {
    cursor: CommonCursor<G>,
    list: &'a RawList<G>,
}

impl<'a, G: GetLinks> Cursor<'a, G> {
    fn new(list: &'a RawList<G>, cur: Option<NonNull<G::EntryType>>) -> Self {
        Self {
            list,
            cursor: CommonCursor::new(cur),
        }
    }

    /// Returns the element the cursor is currently positioned on.
    pub fn current(&self) -> Option<&'a G::EntryType> {
        let cur = self.cursor.cur?;
        // SAFETY: Objects must be kept alive while on the list.
        Some(unsafe { &*cur.as_ptr() })
    }

    /// Moves the cursor to the next element.
    pub fn move_next(&mut self) {
        self.cursor.move_next(self.list);
    }
}

pub(crate) struct CursorMut<'a, G: GetLinks> {
    cursor: CommonCursor<G>,
    list: &'a mut RawList<G>,
}

impl<'a, G: GetLinks> CursorMut<'a, G> {
    fn new(list: &'a mut RawList<G>, cur: Option<NonNull<G::EntryType>>) -> Self {
        Self {
            list,
            cursor: CommonCursor::new(cur),
        }
    }

    pub(crate) fn current(&mut self) -> Option<&mut G::EntryType> {
        let cur = self.cursor.cur?;
        // SAFETY: Objects must be kept alive while on the list.
        Some(unsafe { &mut *cur.as_ptr() })
    }

    /// Removes the entry the cursor is pointing to and advances the cursor to the next entry. It
    /// returns a raw pointer to the removed element (if one is removed).
    pub(crate) fn remove_current(&mut self) -> Option<NonNull<G::EntryType>> {
        let entry = self.cursor.cur?;
        self.cursor.move_next(self.list);
        // SAFETY: The entry is on the list as we just got it from there and it cannot change.
        unsafe { self.list.remove(entry.as_ref()) };
        Some(entry)
    }

    pub(crate) fn peek_next(&mut self) -> Option<&mut G::EntryType> {
        let mut new = CommonCursor::new(self.cursor.cur);
        new.move_next(self.list);
        // SAFETY: Objects must be kept alive while on the list.
        Some(unsafe { &mut *new.cur?.as_ptr() })
    }

    pub(crate) fn peek_prev(&mut self) -> Option<&mut G::EntryType> {
        let mut new = CommonCursor::new(self.cursor.cur);
        new.move_prev(self.list);
        // SAFETY: Objects must be kept alive while on the list.
        Some(unsafe { &mut *new.cur?.as_ptr() })
    }

    pub(crate) fn move_next(&mut self) {
        self.cursor.move_next(self.list);
    }
}
