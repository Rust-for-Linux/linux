// SPDX-License-Identifier: GPL-2.0

//! A reference-counted pointer.
//!
//! This module implements a way for users to create reference-counted objects and pointers to
//! them. Such a pointer automatically increments and decrements the count, and drops the
//! underlying object when it reaches zero. It is also safe to use concurrently from multiple
//! threads.
//!
//! It is different from the standard library's [`Arc`] in a few ways:
//! 1. It is backed by the kernel's `refcount_t` type.
//! 2. It does not support weak references, which allows it to be half the size.
//! 3. It saturates the reference count instead of aborting when it goes over a threshold.
//! 4. It does not provide a `get_mut` method, so the ref counted object is pinned.
//!
//! [`Arc`]: https://doc.rust-lang.org/std/sync/struct.Arc.html

use crate::{
    bindings,
    init::{Init, PinInit},
    prelude::*,
    Error, Opaque, Result,
};
use alloc::{
    alloc::{alloc, dealloc},
    vec::Vec,
};
use core::{
    alloc::Layout,
    convert::{AsRef, TryFrom},
    marker::{PhantomData, Unsize},
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::{self, NonNull},
};

/// A reference-counted pointer to an instance of `T`.
///
/// The reference count is incremented when new instances of [`Arc`] are created, and decremented
/// when they are dropped. When the count reaches zero, the underlying `T` is also dropped.
///
/// # Invariants
///
/// The reference count on an instance of [`Arc`] is always non-zero.
/// The object pointed to by [`Arc`] is always pinned.
pub struct Arc<T: ?Sized> {
    ptr: NonNull<ArcInner<T>>,
    _p: PhantomData<ArcInner<T>>,
}

#[pin_project]
#[repr(C)]
struct ArcInner<T: ?Sized> {
    #[pin]
    refcount: Opaque<bindings::refcount_t>,
    #[pin]
    data: T,
}

// This is to allow [`Arc`] (and variants) to be used as the type of `self`.
impl<T: ?Sized> core::ops::Receiver for Arc<T> {}

// This is to allow [`ArcBorrow`] (and variants) to be used as the type of `self`.
impl<T: ?Sized> core::ops::Receiver for ArcBorrow<'_, T> {}

// This is to allow coercion from `Arc<T>` to `Arc<U>` if `T` can be converted to the
// dynamically-sized type (DST) `U`.
impl<T: ?Sized + Unsize<U>, U: ?Sized> core::ops::CoerceUnsized<Arc<U>> for Arc<T> {}

// This is to allow `Arc<U>` to be dispatched on when `Arc<T>` can be coerced into `Arc<U>`.
impl<T: ?Sized + Unsize<U>, U: ?Sized> core::ops::DispatchFromDyn<Arc<U>> for Arc<T> {}

// SAFETY: It is safe to send `Arc<T>` to another thread when the underlying `T` is `Sync` because
// it effectively means sharing `&T` (which is safe because `T` is `Sync`); additionally, it needs
// `T` to be `Send` because any thread that has a `Arc<T>` may ultimately access `T` directly, for
// example, when the reference count reaches zero and `T` is dropped.
unsafe impl<T: ?Sized + Sync + Send> Send for Arc<T> {}

// SAFETY: It is safe to send `&Arc<T>` to another thread when the underlying `T` is `Sync` for
// the same reason as above. `T` needs to be `Send` as well because a thread can clone a `&Arc<T>`
// into a `Arc<T>`, which may lead to `T` being accessed by the same reasoning as above.
unsafe impl<T: ?Sized + Sync + Send> Sync for Arc<T> {}

impl<T> Arc<T> {
    /// Constructs a new reference counted instance of `T`.
    pub fn try_new(contents: T) -> Result<Self> {
        let layout = Layout::new::<ArcInner<T>>();
        // SAFETY: The layout size is guaranteed to be non-zero because `ArcInner` contains the
        // reference count.
        let inner = NonNull::new(unsafe { alloc(layout) })
            .ok_or(ENOMEM)?
            .cast::<ArcInner<T>>();

        // INVARIANT: The refcount is initialised to a non-zero value.
        let value = ArcInner {
            refcount: Opaque::new(new_refcount()),
            data: contents,
        };
        // SAFETY: `inner` is writable and properly aligned.
        unsafe { inner.as_ptr().write(value) };

        // SAFETY: We just created `inner` with a reference count of 1, which is owned by the new
        // `Arc` object.
        Ok(unsafe { Self::from_inner(inner) })
    }

    /// Creates a new `Arc` and initializes its contenst with the given initializer.
    pub fn pin_init<E>(init: impl PinInit<T, E>) -> Result<Self>
    where
        Error: From<E>,
    {
        UniqueArc::pin_init(init).map(|u| u.into())
    }

    /// Deconstructs a [`Arc`] object into a `usize`.
    ///
    /// It can be reconstructed once via [`Arc::from_usize`].
    pub fn into_usize(obj: Self) -> usize {
        ManuallyDrop::new(obj).ptr.as_ptr() as _
    }

    /// Borrows a [`Arc`] instance previously deconstructed via [`Arc::into_usize`].
    ///
    /// # Safety
    ///
    /// `encoded` must have been returned by a previous call to [`Arc::into_usize`]. Additionally,
    /// [`Arc::from_usize`] can only be called after *all* instances of [`ArcBorrow`] have been
    /// dropped.
    pub unsafe fn borrow_usize<'a>(encoded: usize) -> ArcBorrow<'a, T> {
        // SAFETY: By the safety requirement of this function, we know that `encoded` came from
        // a previous call to `Arc::into_usize`.
        let inner = NonNull::new(encoded as *mut ArcInner<T>).unwrap();

        // SAFETY: The safety requirements ensure that the object remains alive for the lifetime of
        // the returned value. There is no way to create mutable references to the object.
        unsafe { ArcBorrow::new(inner) }
    }

    /// Recreates a [`Arc`] instance previously deconstructed via [`Arc::into_usize`].
    ///
    /// # Safety
    ///
    /// `encoded` must have been returned by a previous call to [`Arc::into_usize`]. Additionally,
    /// it can only be called once for each previous call to [`Arc::into_usize`].
    pub unsafe fn from_usize(encoded: usize) -> Self {
        // SAFETY: By the safety invariants we know that `encoded` came from `Arc::into_usize`, so
        // the reference count held then will be owned by the new `Arc` object.
        unsafe { Self::from_inner(NonNull::new(encoded as _).unwrap()) }
    }
}

impl<T: ?Sized> Arc<T> {
    /// Constructs a new [`Arc`] from an existing [`ArcInner`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that `inner` points to a valid location and has a non-zero reference
    /// count, one of which will be owned by the new [`Arc`] instance.
    unsafe fn from_inner(inner: NonNull<ArcInner<T>>) -> Self {
        // INVARIANT: By the safety requirements, the invariants hold.
        Arc {
            ptr: inner,
            _p: PhantomData,
        }
    }

    /// Determines if two reference-counted pointers point to the same underlying instance of `T`.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        ptr::eq(a.ptr.as_ptr(), b.ptr.as_ptr())
    }

    /// Deconstructs a [`Arc`] object into a raw pointer.
    ///
    /// It can be reconstructed once via [`Arc::from_raw`].
    pub fn into_raw(obj: Self) -> *const T {
        let ret = &*obj as *const T;
        core::mem::forget(obj);
        ret
    }

    /// Recreates a [`Arc`] instance previously deconstructed via [`Arc::into_raw`].
    ///
    /// This code relies on the `repr(C)` layout of structs as described in
    /// <https://doc.rust-lang.org/reference/type-layout.html#reprc-structs>.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by a previous call to [`Arc::into_raw`]. Additionally, it
    /// can only be called once for each previous call to [`Arc::into_raw`].
    pub unsafe fn from_raw(ptr: *const T) -> Self {
        // SAFETY: The safety requirement ensures that the pointer is valid.
        let align = core::mem::align_of_val(unsafe { &*ptr });
        let offset = Layout::new::<ArcInner<()>>()
            .align_to(align)
            .unwrap()
            .pad_to_align()
            .size();
        // SAFETY: The pointer is in bounds because by the safety requirements `ptr` came from
        // `Arc::into_raw`, so it is a pointer `offset` bytes from the beginning of the allocation.
        let data = unsafe { (ptr as *const u8).sub(offset) };
        let metadata = ptr::metadata(ptr as *const ArcInner<T>);
        let ptr = ptr::from_raw_parts_mut(data as _, metadata);
        // SAFETY: By the safety requirements we know that `ptr` came from `Arc::into_raw`, so the
        // reference count held then will be owned by the new `Arc` object.
        unsafe { Self::from_inner(NonNull::new(ptr).unwrap()) }
    }

    /// Returns a [`ArcBorrow`] from the given [`Arc`].
    ///
    /// This is useful when the argument of a function call is a [`ArcBorrow`] (e.g., in a method
    /// receiver), but we have a [`Arc`] instead. Getting a [`ArcBorrow`] is free when optimised.
    #[inline]
    pub fn as_arc_borrow(&self) -> ArcBorrow<'_, T> {
        // SAFETY: The constraint that lifetime of the shared reference must outlive that of
        // the returned `ArcBorrow` ensures that the object remains alive.
        unsafe { ArcBorrow::new(self.ptr) }
    }
}

impl<T: ?Sized> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: By the type invariant, there is necessarily a reference to the object, so it is
        // safe to dereference it.
        unsafe { &self.ptr.as_ref().data }
    }
}

impl<T: ?Sized> Clone for Arc<T> {
    fn clone(&self) -> Self {
        // INVARIANT: C `refcount_inc` saturates the refcount, so it cannot overflow to zero.
        // SAFETY: By the type invariant, there is necessarily a reference to the object, so it is
        // safe to increment the refcount.
        unsafe { bindings::refcount_inc(self.ptr.as_ref().refcount.get()) };

        // SAFETY: We just incremented the refcount. This increment is now owned by the new `Arc`.
        unsafe { Self::from_inner(self.ptr) }
    }
}

impl<T: ?Sized> AsRef<T> for Arc<T> {
    fn as_ref(&self) -> &T {
        // SAFETY: By the type invariant, there is necessarily a reference to the object, so it is
        // safe to dereference it.
        unsafe { &self.ptr.as_ref().data }
    }
}

impl<T: ?Sized> Drop for Arc<T> {
    fn drop(&mut self) {
        // SAFETY: By the type invariant, there is necessarily a reference to the object. We cannot
        // touch `refcount` after it's decremented to a non-zero value because another thread/CPU
        // may concurrently decrement it to zero and free it. It is ok to have a raw pointer to
        // freed/invalid memory as long as it is never dereferenced.
        let refcount = unsafe { self.ptr.as_ref() }.refcount.get();

        // INVARIANT: If the refcount reaches zero, there are no other instances of `Arc`, and
        // this instance is being dropped, so the broken invariant is not observable.
        // SAFETY: Also by the type invariant, we are allowed to decrement the refcount.
        let is_zero = unsafe { bindings::refcount_dec_and_test(refcount) };
        if is_zero {
            // The count reached zero, we must free the memory.

            // SAFETY: This thread holds the only remaining reference to `self`, so it is safe to
            // get a mutable reference to it.
            let inner = unsafe { self.ptr.as_mut() };
            let layout = Layout::for_value(inner);
            // SAFETY: The value stored in inner is valid.
            unsafe { core::ptr::drop_in_place(inner) };
            // SAFETY: The pointer was initialised from the result of a call to `alloc`.
            unsafe { dealloc(self.ptr.cast().as_ptr(), layout) };
        }
    }
}

impl<T> TryFrom<Vec<T>> for Arc<[T]> {
    type Error = Error;

    fn try_from(mut v: Vec<T>) -> Result<Self> {
        let value_layout = Layout::array::<T>(v.len())?;
        let layout = Layout::new::<ArcInner<()>>()
            .extend(value_layout)?
            .0
            .pad_to_align();
        // SAFETY: The layout size is guaranteed to be non-zero because `ArcInner` contains the
        // reference count.
        let ptr = NonNull::new(unsafe { alloc(layout) }).ok_or(ENOMEM)?;
        let inner =
            core::ptr::slice_from_raw_parts_mut(ptr.as_ptr() as _, v.len()) as *mut ArcInner<[T]>;

        // SAFETY: Just an FFI call that returns a `refcount_t` initialised to 1.
        let count = Opaque::new(unsafe { bindings::REFCOUNT_INIT(1) });
        // SAFETY: `inner.refcount` is writable and properly aligned.
        unsafe { core::ptr::addr_of_mut!((*inner).refcount).write(count) };
        // SAFETY: The contents of `v` as readable and properly aligned; `inner.data` is writable
        // and properly aligned. There is no overlap between the two because `inner` is a new
        // allocation.
        unsafe {
            core::ptr::copy_nonoverlapping(
                v.as_ptr(),
                core::ptr::addr_of_mut!((*inner).data) as *mut [T] as *mut T,
                v.len(),
            )
        };
        // SAFETY: We're setting the new length to zero, so it is <= to capacity, and old_len..0 is
        // an empty range (so satisfies vacuously the requirement of being initialised).
        unsafe { v.set_len(0) };
        // SAFETY: We just created `inner` with a reference count of 1, which is owned by the new
        // `Arc` object.
        Ok(unsafe { Self::from_inner(NonNull::new(inner).unwrap()) })
    }
}

impl<T: ?Sized> From<UniqueArc<T>> for Arc<T> {
    fn from(item: UniqueArc<T>) -> Self {
        item.inner
    }
}

impl<T: ?Sized> From<UniqueArc<T>> for Pin<UniqueArc<T>> {
    fn from(obj: UniqueArc<T>) -> Self {
        // SAFETY: It is not possible to move/replace `T` inside a `Pin<UniqueArc<T>>` (unless `T`
        // is `Unpin`), so it is ok to convert it to `Pin<UniqueArc<T>>`.
        unsafe { Pin::new_unchecked(obj) }
    }
}

impl<T: ?Sized> From<Pin<UniqueArc<T>>> for Arc<T> {
    fn from(item: Pin<UniqueArc<T>>) -> Self {
        // SAFETY: The type invariants of `Arc` guarantee that the data is pinned.
        unsafe { Pin::into_inner_unchecked(item).inner }
    }
}

/// A borrowed [`Arc`] with manually-managed lifetime.
///
/// # Invariants
///
/// There are no mutable references to the underlying [`Arc`], and it remains valid for the lifetime
/// of the [`ArcBorrow`] instance.
pub struct ArcBorrow<'a, T: ?Sized + 'a> {
    inner: NonNull<ArcInner<T>>,
    _p: PhantomData<&'a ()>,
}

impl<T: ?Sized> Clone for ArcBorrow<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for ArcBorrow<'_, T> {}

impl<T: ?Sized> ArcBorrow<'_, T> {
    /// Creates a new [`ArcBorrow`] instance.
    ///
    /// # Safety
    ///
    /// Callers must ensure the following for the lifetime of the returned [`ArcBorrow`] instance:
    /// 1. That `obj` remains valid;
    /// 2. That no mutable references to `obj` are created.
    unsafe fn new(inner: NonNull<ArcInner<T>>) -> Self {
        // INVARIANT: The safety requirements guarantee the invariants.
        Self {
            inner,
            _p: PhantomData,
        }
    }
}

impl<T: ?Sized> From<ArcBorrow<'_, T>> for Arc<T> {
    fn from(b: ArcBorrow<'_, T>) -> Self {
        // SAFETY: The existence of `b` guarantees that the refcount is non-zero. `ManuallyDrop`
        // guarantees that `drop` isn't called, so it's ok that the temporary `Arc` doesn't own the
        // increment.
        ManuallyDrop::new(unsafe { Arc::from_inner(b.inner) })
            .deref()
            .clone()
    }
}

impl<T: ?Sized> Deref for ArcBorrow<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: By the type invariant, the underlying object is still alive with no mutable
        // references to it, so it is safe to create a shared reference.
        unsafe { &self.inner.as_ref().data }
    }
}

/// A refcounted object that is known to have a refcount of 1.
///
/// It is mutable and can be converted to a [`Arc`] so that it can be shared.
///
/// # Invariants
///
/// `inner` always has a reference count of 1.
///
/// # Examples
///
/// In the following example, we make changes to the inner object before turning it into a
/// `Arc<Test>` object (after which point, it cannot be mutated directly). Note that `x.into()`
/// cannot fail.
///
/// ```
/// use kernel::sync::{Arc, UniqueArc};
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn test() -> Result<Arc<Example>> {
///     let mut x = UniqueArc::try_new(Example { a: 10, b: 20 })?;
///     x.a += 1;
///     x.b += 1;
///     Ok(x.into())
/// }
///
/// # test();
/// ```
///
/// In the following example we first allocate memory for a ref-counted `Example` but we don't
/// initialise it on allocation. We do initialise it later with a call to [`UniqueArc::write`],
/// followed by a conversion to `Arc<Example>`. This is particularly useful when allocation happens
/// in one context (e.g., sleepable) and initialisation in another (e.g., atomic):
///
/// ```
/// use kernel::sync::{Arc, UniqueArc};
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn test() -> Result<Arc<Example>> {
///     let x = UniqueArc::try_new_uninit()?;
///     Ok(x.write(Example { a: 10, b: 20 }).into())
/// }
///
/// # test();
/// ```
///
/// In the last example below, the caller gets a pinned instance of `Example` while converting to
/// `Arc<Example>`; this is useful in scenarios where one needs a pinned reference during
/// initialisation, for example, when initialising fields that are wrapped in locks.
///
/// ```
/// use kernel::sync::{Arc, UniqueArc};
///
/// struct Example {
///     a: u32,
///     b: u32,
/// }
///
/// fn test() -> Result<Arc<Example>> {
///     let mut pinned = Pin::from(UniqueArc::try_new(Example { a: 10, b: 20 })?);
///     // We can modify `pinned` because it is `Unpin`.
///     pinned.as_mut().a += 1;
///     Ok(pinned.into())
/// }
///
/// # test();
/// ```
pub struct UniqueArc<T: ?Sized> {
    inner: Arc<T>,
}

impl<T> UniqueArc<T> {
    /// Tries to allocate a new [`UniqueArc`] instance.
    pub fn try_new(value: T) -> Result<Self> {
        Ok(Self {
            // INVARIANT: The newly-created object has a ref-count of 1.
            inner: Arc::try_new(value)?,
        })
    }

    /// Tries to allocate a new [`UniqueArc`] instance whose contents are not initialised yet.
    pub fn try_new_uninit() -> Result<UniqueArc<MaybeUninit<T>>> {
        let layout = Layout::new::<ArcInner<MaybeUninit<T>>>();
        // SAFETY: The layout size is guaranteed to be non-zero because `ArcInner` contains the
        // reference count.
        let inner = NonNull::new(unsafe { alloc(layout) })
            .ok_or(ENOMEM)?
            .cast::<ArcInner<MaybeUninit<T>>>();
        let init = pin_init!(ArcInner<MaybeUninit<T>> {
            // INVARIANT: The refcount is initialised to a non-zero value.
            refcount: Opaque::new(new_refcount()),
            data: init::uninit(),
        });
        // SAFETY: the pointer is valid, as we just allocated it. It will also stay pinned (type
        // invariant).
        match unsafe { init.__pinned_init(inner.as_ptr()) } {
            Ok(()) => {}
            Err(e) => match e {},
        }

        // SAFETY: We just created `inner` with a reference count of 1, which is owned by the new
        // `Arc` object.
        Ok(UniqueArc::<MaybeUninit<T>> {
            // INVARIANT: The newly-created object has a ref-count of 1.
            inner: unsafe { Arc::from_inner(inner) },
        })
    }
}

impl<T> UniqueArc<MaybeUninit<T>> {
    /// Converts a `UniqueArc<MaybeUninit<T>>` into a `UniqueArc<T>` by writing a value into it.
    pub fn write(mut self, value: T) -> UniqueArc<T> {
        self.deref_mut().write(value);
        // SAFETY: we just initialized `self`
        unsafe { self.assume_init() }
    }

    /// Initialize the value after creating the `UniqueArc`.
    pub fn pin_init_now<E>(
        mut self,
        init: impl PinInit<T, E>,
    ) -> core::result::Result<Pin<UniqueArc<T>>, (E, Self)> {
        unsafe {
            match init.__pinned_init(self.deref_mut().as_mut_ptr()) {
                Ok(()) => Ok(Pin::from(self.assume_init())),
                Err(e) => Err((e, self)),
            }
        }
    }

    /// Initialize the value after creating the `UniqueArc`.
    pub fn init_now<E>(
        mut self,
        init: impl Init<T, E>,
    ) -> core::result::Result<UniqueArc<T>, (E, UniqueArc<MaybeUninit<T>>)> {
        unsafe {
            match init.__init(self.deref_mut().as_mut_ptr()) {
                Ok(()) => Ok(self.assume_init()),
                Err(e) => Err((e, self)),
            }
        }
    }

    /// Unsafely assume that `self` is initialized.
    ///
    /// # Safety
    ///
    /// The caller guarantees that the value behind this pointer has been initialized. It is
    /// *immediate* UB to call this when the value is not initialized.
    pub unsafe fn assume_init(self) -> UniqueArc<T> {
        let inner = ManuallyDrop::new(self).inner.ptr;
        UniqueArc {
            // SAFETY: The new `Arc` is taking over `ptr` from `self.inner` (which won't be
            // dropped). The types are compatible because `MaybeUninit<T>` is compatible with `T`.
            inner: unsafe { Arc::from_inner(inner.cast()) },
        }
    }
}

impl<T: ?Sized> Deref for UniqueArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<T: ?Sized> DerefMut for UniqueArc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: By the `Arc` type invariant, there is necessarily a reference to the object, so
        // it is safe to dereference it. Additionally, we know there is only one reference when
        // it's inside a `UniqueArc`, so it is safe to get a mutable reference.
        unsafe { &mut self.inner.ptr.as_mut().data }
    }
}

/// Allows the creation of "reference-counted" globals.
///
/// This is achieved by biasing the refcount with +1, which ensures that the count never drops back
/// to zero (unless buggy unsafe code incorrectly decrements without owning an increment) and
/// therefore also ensures that `drop` is never called.
///
/// # Examples
///
/// ```
/// use kernel::sync::{Arc, ArcBorrow, StaticArc};
///
/// const VALUE: u32 = 10;
/// static SR: StaticArc<u32> = StaticArc::new(VALUE);
///
/// fn takes_ref_borrow(v: ArcBorrow<'_, u32>) {
///     assert_eq!(*v, VALUE);
/// }
///
/// fn takes_ref(v: Arc<u32>) {
///     assert_eq!(*v, VALUE);
/// }
///
/// takes_ref_borrow(SR.as_arc_borrow());
/// takes_ref(SR.as_arc_borrow().into());
/// ```
pub struct StaticArc<T: ?Sized> {
    inner: ArcInner<T>,
}

// SAFETY: A `StaticArc<T>` is a `Arc<T>` declared statically, so we just use the same criteria for
// making it `Sync`.
unsafe impl<T: ?Sized + Sync + Send> Sync for StaticArc<T> {}

impl<T> StaticArc<T> {
    /// Creates a new instance of a static "ref-counted" object.
    pub const fn new(data: T) -> Self {
        // INVARIANT: The refcount is initialised to a non-zero value.
        Self {
            inner: ArcInner {
                refcount: Opaque::new(new_refcount()),
                data,
            },
        }
    }
}

impl<T: ?Sized> StaticArc<T> {
    /// Creates a [`ArcBorrow`] instance from the given static object.
    ///
    /// This requires a `'static` lifetime so that it can guarantee that the underlyling object
    /// remains valid and is effectively pinned.
    pub fn as_arc_borrow(&'static self) -> ArcBorrow<'static, T> {
        // SAFETY: The static lifetime guarantees that the object remains valid. And the shared
        // reference guarantees that no mutable references exist.
        unsafe { ArcBorrow::new(NonNull::from(&self.inner)) }
    }
}

/// Creates, from a const context, a new instance of `struct refcount_struct` with a refcount of 1.
///
/// ```
/// # // The test below is meant to ensure that `new_refcount` (which is const) mimics
/// # // `REFCOUNT_INIT`, which is written in C and thus can't be used in a const context.
/// # // TODO: Once `#[test]` is working, move this to a test and make `new_refcount` private.
/// # use kernel::bindings;
/// # // SAFETY: Just an FFI call that returns a `refcount_t` initialised to 1.
/// # let bindings::refcount_struct {
/// #     refs: bindings::atomic_t { counter: a },
/// # } = unsafe { bindings::REFCOUNT_INIT(1) };
/// # let bindings::refcount_struct {
/// #     refs: bindings::atomic_t { counter: b },
/// # } = kernel::sync::new_refcount();
/// # assert_eq!(a, b);
/// ```
pub const fn new_refcount() -> bindings::refcount_struct {
    bindings::refcount_struct {
        refs: bindings::atomic_t { counter: 1 },
    }
}
