// SPDX-License-Identifier: GPL-2.0

//! Intrusive circular doubly-linked lists.
//!
//! We don't use the C version for two main reasons:
//! - Next/prev pointers do not support `?Sized` types, so wouldn't be able to have a list of, for
//!    example, dyn Trait.
//! - It would require the list head to be pinned (in addition to the list entries).

use core::{cell::UnsafeCell, iter, marker::PhantomPinned, mem::MaybeUninit, ptr::NonNull};

/// An intrusive circular doubly-linked list.
///
/// Membership of elements of the list must be tracked by the owner of the list.
///
/// While elements of the list must remain pinned while in the list, the list itself does not
/// require pinning. In other words, users are allowed to move instances of [`List`].
///
/// # Invariants
///
/// The links of an entry are wrapped in [`UnsafeCell`] and they are acessible when the list itself
/// is. For example, when a thread has a mutable reference to a list, it may also safely get
/// mutable references to the links of the elements in the list.
///
/// The links of an entry are also wrapped in [`MaybeUninit`] and they are initialised when they
/// are present in a list. Otherwise they are uninitialised.
///
/// # Examples
///
/// ```
/// # use kernel::unsafe_list::{Adapter, Links, List};
///
/// struct Example {
///     v: usize,
///     links: Links<Example>,
/// }
///
/// // SAFETY: This adapter is the only one that uses `Example::links`.
/// unsafe impl Adapter for Example {
///     type EntryType = Self;
///     fn to_links(obj: &Self) -> &Links<Self> {
///         &obj.links
///     }
/// }
///
/// let a = Example {
///     v: 0,
///     links: Links::new(),
/// };
/// let b = Example {
///     v: 1,
///     links: Links::new(),
/// };
///
/// let mut list = List::<Example>::new();
/// assert!(list.is_empty());
///
/// // SAFETY: `a` was declared above, it's not in any lists yet, is never moved, and outlives the
/// // list.
/// unsafe { list.push_back(&a) };
///
/// // SAFETY: `b` was declared above, it's not in any lists yet, is never moved, and outlives the
/// // list.
/// unsafe { list.push_back(&b) };
///
/// assert!(core::ptr::eq(&a, list.front().unwrap().as_ptr()));
/// assert!(core::ptr::eq(&b, list.back().unwrap().as_ptr()));
///
/// for (i, e) in list.iter().enumerate() {
///     assert_eq!(i, e.v);
/// }
///
/// for e in &list {
///     pr_info!("{}", e.v);
/// }
///
/// // SAFETY: `b` was added to the list above and wasn't removed yet.
/// unsafe { list.remove(&b) };
///
/// assert!(core::ptr::eq(&a, list.front().unwrap().as_ptr()));
/// assert!(core::ptr::eq(&a, list.back().unwrap().as_ptr()));
/// ```
pub struct List<A: Adapter + ?Sized> {
    first: Option<NonNull<A::EntryType>>,
}

// SAFETY: The list is itself can be safely sent to other threads but we restrict it to being `Send`
// only when its entries are also `Send`.
unsafe impl<A: Adapter + ?Sized> Send for List<A> where A::EntryType: Send {}

// SAFETY: The list is itself usable from other threads via references but we restrict it to being
// `Sync` only when its entries are also `Sync`.
unsafe impl<A: Adapter + ?Sized> Sync for List<A> where A::EntryType: Sync {}

impl<A: Adapter + ?Sized> List<A> {
    /// Constructs a new empty list.
    pub const fn new() -> Self {
        Self { first: None }
    }

    /// Determines if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    /// Inserts the only entry to a list.
    ///
    /// This must only be called when the list is empty.
    pub fn insert_only_entry(&mut self, obj: &A::EntryType) {
        let obj_ptr = NonNull::from(obj);

        // SAFETY: We have mutable access to the list, so we also have access to the entry
        // we're about to insert (and it's not in any other lists per the function safety
        // requirements).
        let obj_inner = unsafe { &mut *A::to_links(obj).0.get() };

        // INVARIANTS: All fields of the links of the newly-inserted object are initialised
        // below.
        obj_inner.write(LinksInner {
            next: obj_ptr,
            prev: obj_ptr,
            _pin: PhantomPinned,
        });
        self.first = Some(obj_ptr);
    }

    /// Adds the given object to the end of the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    /// - The object is not currently in any lists.
    /// - The object remains alive until it is removed from the list.
    /// - The object is not moved until it is removed from the list.
    pub unsafe fn push_back(&mut self, obj: &A::EntryType) {
        if let Some(first) = self.first {
            // SAFETY: The previous entry to the first one is necessarily present in the list (it
            // may in fact be the first entry itself as this is a circular list). The safety
            // requirements of this function regarding `obj` satisfy those of `insert_after`.
            unsafe { self.insert_after(self.inner_ref(first).prev, obj) };
        } else {
            self.insert_only_entry(obj);
        }
    }

    /// Adds the given object to the beginning of the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    /// - The object is not currently in any lists.
    /// - The object remains alive until it is removed from the list.
    /// - The object is not moved until it is removed from the list.
    pub unsafe fn push_front(&mut self, obj: &A::EntryType) {
        if let Some(first) = self.first {
            // SAFETY: The safety requirements of this function regarding `obj` satisfy those of
            // `insert_before`. Additionally, `first` is in the list.
            unsafe { self.insert_before(first, obj) };
        } else {
            self.insert_only_entry(obj);
        }
    }

    /// Removes the given object from the list.
    ///
    /// # Safety
    ///
    /// The object must be in the list. In other words, the object must have previously been
    /// inserted into this list and not removed yet.
    pub unsafe fn remove(&mut self, entry: &A::EntryType) {
        // SAFETY: Per the function safety requirements, `entry` is in the list.
        let inner = unsafe { self.inner_ref(NonNull::from(entry)) };
        let next = inner.next;
        let prev = inner.prev;

        // SAFETY: We have mutable access to the list, so we also have access to the entry we're
        // about to remove (which we know is in the list per the function safety requirements).
        let inner = unsafe { &mut *A::to_links(entry).0.get() };

        // SAFETY: Since the entry was in the list, it was initialised.
        unsafe { inner.assume_init_drop() };

        if core::ptr::eq(next.as_ptr(), entry) {
            // Removing the only element.
            self.first = None;
        } else {
            // SAFETY: `prev` is in the list because it is pointed at by the entry being removed.
            unsafe { self.inner(prev).next = next };
            // SAFETY: `next` is in the list because it is pointed at by the entry being removed.
            unsafe { self.inner(next).prev = prev };

            if core::ptr::eq(self.first.unwrap().as_ptr(), entry) {
                // Update the pointer to the first element as we're removing it.
                self.first = Some(next);
            }
        }
    }

    /// Adds the given object after another object already in the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    /// - The existing object is currently in the list.
    /// - The new object is not currently in any lists.
    /// - The new object remains alive until it is removed from the list.
    /// - The new object is not moved until it is removed from the list.
    pub unsafe fn insert_after(&mut self, existing: NonNull<A::EntryType>, new: &A::EntryType) {
        // SAFETY: We have mutable access to the list, so we also have access to the entry we're
        // about to insert (and it's not in any other lists per the function safety requirements).
        let new_inner = unsafe { &mut *A::to_links(new).0.get() };

        // SAFETY: Per the function safety requirements, `existing` is in the list.
        let existing_inner = unsafe { self.inner(existing) };
        let next = existing_inner.next;

        // INVARIANTS: All fields of the links of the newly-inserted object are initialised below.
        new_inner.write(LinksInner {
            next,
            prev: existing,
            _pin: PhantomPinned,
        });

        existing_inner.next = NonNull::from(new);

        // SAFETY: `next` is in the list because it's pointed at by the existing entry.
        unsafe { self.inner(next).prev = NonNull::from(new) };
    }

    /// Adds the given object before another object already in the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that:
    /// - The existing object is currently in the list.
    /// - The new object is not currently in any lists.
    /// - The new object remains alive until it is removed from the list.
    /// - The new object is not moved until it is removed from the list.
    pub unsafe fn insert_before(&mut self, existing: NonNull<A::EntryType>, new: &A::EntryType) {
        // SAFETY: The safety requirements of this function satisfy those of `insert_after`.
        unsafe { self.insert_after(self.inner_ref(existing).prev, new) };

        if self.first.unwrap() == existing {
            // Update the pointer to the first element as we're inserting before it.
            self.first = Some(NonNull::from(new));
        }
    }

    /// Returns the first element of the list, if one exists.
    pub fn front(&self) -> Option<NonNull<A::EntryType>> {
        self.first
    }

    /// Returns the last element of the list, if one exists.
    pub fn back(&self) -> Option<NonNull<A::EntryType>> {
        // SAFETY: Having a pointer to it guarantees that the object is in the list.
        self.first.map(|f| unsafe { self.inner_ref(f).prev })
    }

    /// Returns an iterator for the list starting at the first entry.
    pub fn iter(&self) -> Iterator<'_, A> {
        Iterator::new(self.cursor_front())
    }

    /// Returns an iterator for the list starting at the last entry.
    pub fn iter_back(&self) -> impl iter::DoubleEndedIterator<Item = &'_ A::EntryType> {
        Iterator::new(self.cursor_back())
    }

    /// Returns a cursor starting on the first (front) element of the list.
    pub fn cursor_front(&self) -> Cursor<'_, A> {
        // SAFETY: `front` is in the list (or is `None`) because we've read it from the list head
        // and the list cannot have changed because we hold a shared reference to it.
        unsafe { Cursor::new(self, self.front()) }
    }

    /// Returns a cursor starting on the last (back) element of the list.
    pub fn cursor_back(&self) -> Cursor<'_, A> {
        // SAFETY: `back` is in the list (or is `None`) because we've read it from the list head
        // and the list cannot have changed because we hold a shared reference to it.
        unsafe { Cursor::new(self, self.back()) }
    }

    /// Returns a mutable reference to the links of a given object.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the element is in the list.
    unsafe fn inner(&mut self, ptr: NonNull<A::EntryType>) -> &mut LinksInner<A::EntryType> {
        // SAFETY: The safety requirements guarantee that we the links are initialised because
        // that's part of the type invariants. Additionally, the type invariants also guarantee
        // that having a mutable reference to the list guarantees that the links are mutably
        // accessible as well.
        unsafe { (*A::to_links(ptr.as_ref()).0.get()).assume_init_mut() }
    }

    /// Returns a shared reference to the links of a given object.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the element is in the list.
    unsafe fn inner_ref(&self, ptr: NonNull<A::EntryType>) -> &LinksInner<A::EntryType> {
        // SAFETY: The safety requirements guarantee that we the links are initialised because
        // that's part of the type invariants. Additionally, the type invariants also guarantee
        // that having a shared reference to the list guarantees that the links are accessible in
        // shared mode as well.
        unsafe { (*A::to_links(ptr.as_ref()).0.get()).assume_init_ref() }
    }
}

impl<'a, A: Adapter + ?Sized> iter::IntoIterator for &'a List<A> {
    type Item = &'a A::EntryType;
    type IntoIter = Iterator<'a, A>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator for the linked list.
pub struct Iterator<'a, A: Adapter + ?Sized> {
    cursor: Cursor<'a, A>,
}

impl<'a, A: Adapter + ?Sized> Iterator<'a, A> {
    fn new(cursor: Cursor<'a, A>) -> Self {
        Self { cursor }
    }
}

impl<'a, A: Adapter + ?Sized> iter::Iterator for Iterator<'a, A> {
    type Item = &'a A::EntryType;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.cursor.current()?;
        self.cursor.move_next();
        Some(ret)
    }
}

impl<A: Adapter + ?Sized> iter::DoubleEndedIterator for Iterator<'_, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let ret = self.cursor.current()?;
        self.cursor.move_prev();
        Some(ret)
    }
}

/// A linked-list adapter.
///
/// It is a separate type (as opposed to implemented by the type of the elements of the list)
/// so that a given type can be inserted into multiple lists at the same time; in such cases, each
/// list needs its own adapter that returns a different pointer to links.
///
/// It may, however, be implemented by the type itself to be inserted into lists, which makes it
/// more readable.
///
/// # Safety
///
/// Implementers must ensure that the links returned by [`Adapter::to_links`] are unique to the
/// adapter. That is, different adapters must return different links for a given object.
///
/// The reason for this requirement is to avoid confusion that may lead to UB. In particular, if
/// two adapters were to use the same links, a user may have two lists (one for each adapter) and
/// try to insert the same object into both at the same time; although this clearly violates the
/// list safety requirements (e.g., those in [`List::push_back`]), for users to notice it, they'd
/// have to dig into the details of the two adapters.
///
/// By imposing the requirement on the adapter, we make it easier for users to check compliance
/// with the requirements when using the list.
///
/// # Examples
///
/// ```
/// # use kernel::unsafe_list::{Adapter, Links, List};
///
/// struct Example {
///     a: u32,
///     b: u32,
///     links1: Links<Example>,
///     links2: Links<Example>,
/// }
///
/// // SAFETY: This adapter is the only one that uses `Example::links1`.
/// unsafe impl Adapter for Example {
///     type EntryType = Self;
///     fn to_links(obj: &Self) -> &Links<Self> {
///         &obj.links1
///     }
/// }
///
/// struct ExampleAdapter;
///
/// // SAFETY: This adapter is the only one that uses `Example::links2`.
/// unsafe impl Adapter for ExampleAdapter {
///     type EntryType = Example;
///     fn to_links(obj: &Example) -> &Links<Example> {
///         &obj.links2
///     }
/// }
///
/// static LIST1: List<Example> = List::new();
/// static LIST2: List<ExampleAdapter> = List::new();
/// ```
pub unsafe trait Adapter {
    /// The type of the enties in the list.
    type EntryType: ?Sized;

    /// Retrieves the linked list links for the given object.
    fn to_links(obj: &Self::EntryType) -> &Links<Self::EntryType>;
}

struct LinksInner<T: ?Sized> {
    next: NonNull<T>,
    prev: NonNull<T>,
    _pin: PhantomPinned,
}

/// Links of a linked list.
///
/// List entries need one of these per concurrent list.
pub struct Links<T: ?Sized>(UnsafeCell<MaybeUninit<LinksInner<T>>>);

// SAFETY: `Links` can be safely sent to other threads but we restrict it to being `Send` only when
// the list entries it points to are also `Send`.
unsafe impl<T: ?Sized> Send for Links<T> {}

// SAFETY: `Links` is usable from other threads via references but we restrict it to being `Sync`
// only when the list entries it points to are also `Sync`.
unsafe impl<T: ?Sized> Sync for Links<T> {}

impl<T: ?Sized> Links<T> {
    /// Constructs a new instance of the linked-list links.
    pub const fn new() -> Self {
        Self(UnsafeCell::new(MaybeUninit::uninit()))
    }
}

pub(crate) struct CommonCursor<A: Adapter + ?Sized> {
    pub(crate) cur: Option<NonNull<A::EntryType>>,
}

impl<A: Adapter + ?Sized> CommonCursor<A> {
    pub(crate) fn new(cur: Option<NonNull<A::EntryType>>) -> Self {
        Self { cur }
    }

    /// Moves the cursor to the next entry of the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the cursor is either [`None`] or points to an entry that is in
    /// `list`.
    pub(crate) unsafe fn move_next(&mut self, list: &List<A>) {
        match self.cur.take() {
            None => self.cur = list.first,
            Some(cur) => {
                if let Some(head) = list.first {
                    // SAFETY: Per the function safety requirements, `cur` is in the list.
                    let links = unsafe { list.inner_ref(cur) };
                    if links.next != head {
                        self.cur = Some(links.next);
                    }
                }
            }
        }
    }

    /// Moves the cursor to the previous entry of the list.
    ///
    /// # Safety
    ///
    /// Callers must ensure that the cursor is either [`None`] or points to an entry that is in
    /// `list`.
    pub(crate) unsafe fn move_prev(&mut self, list: &List<A>) {
        match list.first {
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
                // SAFETY: `next` is either `head` or `cur`. The former is in the list because it's
                // its head; the latter is in the list per the function safety requirements.
                self.cur = Some(unsafe { list.inner_ref(next) }.prev);
            }
        }
    }
}

/// A list cursor that allows traversing a linked list and inspecting elements.
pub struct Cursor<'a, A: Adapter + ?Sized> {
    cursor: CommonCursor<A>,
    list: &'a List<A>,
}

impl<'a, A: Adapter + ?Sized> Cursor<'a, A> {
    /// Creates a new cursor.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `cur` is either [`None`] or points to an entry in `list`.
    pub(crate) unsafe fn new(list: &'a List<A>, cur: Option<NonNull<A::EntryType>>) -> Self {
        Self {
            list,
            cursor: CommonCursor::new(cur),
        }
    }

    /// Returns the element the cursor is currently positioned on.
    pub fn current(&self) -> Option<&'a A::EntryType> {
        let cur = self.cursor.cur?;
        // SAFETY: `cursor` starts off in the list and only changes within the list. Additionally,
        // the list cannot change because we hold a shared reference to it, so the cursor is always
        // within the list.
        Some(unsafe { cur.as_ref() })
    }

    /// Moves the cursor to the next element.
    pub fn move_next(&mut self) {
        // SAFETY: `cursor` starts off in the list and only changes within the list. Additionally,
        // the list cannot change because we hold a shared reference to it, so the cursor is always
        // within the list.
        unsafe { self.cursor.move_next(self.list) };
    }

    /// Moves the cursor to the previous element.
    pub fn move_prev(&mut self) {
        // SAFETY: `cursor` starts off in the list and only changes within the list. Additionally,
        // the list cannot change because we hold a shared reference to it, so the cursor is always
        // within the list.
        unsafe { self.cursor.move_prev(self.list) };
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, vec::Vec};
    use core::ptr::NonNull;

    struct Example {
        links: super::Links<Self>,
    }

    // SAFETY: This is the only adapter that uses `Example::links`.
    unsafe impl super::Adapter for Example {
        type EntryType = Self;
        fn to_links(obj: &Self) -> &super::Links<Self> {
            &obj.links
        }
    }

    fn build_vector(size: usize) -> Vec<Box<Example>> {
        let mut v = Vec::new();
        v.reserve(size);
        for _ in 0..size {
            v.push(Box::new(Example {
                links: super::Links::new(),
            }));
        }
        v
    }

    #[track_caller]
    fn assert_list_contents(v: &[Box<Example>], list: &super::List<Example>) {
        let n = v.len();

        // Assert that the list is ok going forward.
        let mut count = 0;
        for (i, e) in list.iter().enumerate() {
            assert!(core::ptr::eq(e, &*v[i]));
            count += 1;
        }
        assert_eq!(count, n);

        // Assert that the list is ok going backwards.
        let mut count = 0;
        for (i, e) in list.iter_back().rev().enumerate() {
            assert!(core::ptr::eq(e, &*v[n - 1 - i]));
            count += 1;
        }
        assert_eq!(count, n);
    }

    #[track_caller]
    fn test_each_element(
        min_len: usize,
        max_len: usize,
        test: impl Fn(&mut Vec<Box<Example>>, &mut super::List<Example>, usize, Box<Example>),
    ) {
        for n in min_len..=max_len {
            for i in 0..n {
                let extra = Box::new(Example {
                    links: super::Links::new(),
                });
                let mut v = build_vector(n);
                let mut list = super::List::<Example>::new();

                // Build list.
                for j in 0..n {
                    // SAFETY: The entry was allocated above, it's not in any lists yet, is never
                    // moved, and outlives the list.
                    unsafe { list.push_back(&v[j]) };
                }

                // Call the test case.
                test(&mut v, &mut list, i, extra);

                // Check that the list is ok.
                assert_list_contents(&v, &list);
            }
        }
    }

    #[test]
    fn test_push_back() {
        const MAX: usize = 10;
        let v = build_vector(MAX);
        let mut list = super::List::<Example>::new();

        for n in 1..=MAX {
            // SAFETY: The entry was allocated above, it's not in any lists yet, is never moved,
            // and outlives the list.
            unsafe { list.push_back(&v[n - 1]) };
            assert_list_contents(&v[..n], &list);
        }
    }

    #[test]
    fn test_push_front() {
        const MAX: usize = 10;
        let v = build_vector(MAX);
        let mut list = super::List::<Example>::new();

        for n in 1..=MAX {
            // SAFETY: The entry was allocated above, it's not in any lists yet, is never moved,
            // and outlives the list.
            unsafe { list.push_front(&v[MAX - n]) };
            assert_list_contents(&v[MAX - n..], &list);
        }
    }

    #[test]
    fn test_one_removal() {
        test_each_element(1, 10, |v, list, i, _| {
            // Remove the i-th element.
            // SAFETY: The i-th element was added to the list above, and wasn't removed yet.
            unsafe { list.remove(&v[i]) };
            v.remove(i);
        });
    }

    #[test]
    fn test_one_insert_after() {
        test_each_element(1, 10, |v, list, i, extra| {
            // Insert after the i-th element.
            // SAFETY: The i-th element was added to the list above, and wasn't removed yet.
            // Additionally, the new element isn't in any list yet, isn't moved, and outlives
            // the list.
            unsafe { list.insert_after(NonNull::from(&*v[i]), &*extra) };
            v.insert(i + 1, extra);
        });
    }

    #[test]
    fn test_one_insert_before() {
        test_each_element(1, 10, |v, list, i, extra| {
            // Insert before the i-th element.
            // SAFETY: The i-th element was added to the list above, and wasn't removed yet.
            // Additionally, the new element isn't in any list yet, isn't moved, and outlives
            // the list.
            unsafe { list.insert_before(NonNull::from(&*v[i]), &*extra) };
            v.insert(i, extra);
        });
    }
}
