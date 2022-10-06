// SPDX-License-Identifier: GPL-2.0

//! API to safely and fallibly initialize pinned structs using in-place constructors.
//!
//! It also allows in-place initialization of big structs that would otherwise produce a stack overflow.
//!
//! Most structs from the [sync] module need to be pinned, because they contain self referential
//! structs from C. [Pinning][pinning] is Rust's way of ensuring data does not move.
//!
//! # Overview
//!
//! To initialize a struct with an in-place constructor you will need two things:
//! - an in-place constructor,
//! - a memory location that can hold your struct (this can be the [stack], an [`Arc<T>`],
//!   [`UniqueArc<T>`], [`Box<T>`] or any other smart pointer [^1]).
//!
//! To get an in-place constructor there are generally two options:
//! - directly creating an in-place constructor,
//! - a function/macro returning an in-place constructor.
//!
//! # Examples
//!
//! ## Directly creating an in-place constructor
//!
//! If you want to use [`PinInit`], then you will have to annotate your struct with [`#[pin_project]`].
//! It is a macro that uses `#[pin]` as a marker for [structurally pinned fields].
//!
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! use kernel::{prelude::*, sync::Mutex, new_mutex};
//! # use core::pin::Pin;
//! #[pin_project]
//! struct Foo {
//!     #[pin]
//!     a: Mutex<usize>,
//!     b: u32,
//! }
//!
//! let foo = pin_init!(Foo {
//!     a: new_mutex!(42, "Foo::a"),
//!     b: 24,
//! });
//! # let foo: Result<Pin<Box<Foo>>> = Box::pin_init::<core::convert::Infallible>(foo);
//! ```
//!
//! `foo` now is of the type `impl`[`PinInit<Foo>`]. We can now use any smart pointer that we like
//! (or just the stack) to actually initialize a `Foo`:
//!
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! # use kernel::{prelude::*, sync::Mutex, new_mutex};
//! # use core::pin::Pin;
//! # #[pin_project]
//! # struct Foo {
//! #     #[pin]
//! #     a: Mutex<usize>,
//! #     b: u32,
//! # }
//! # let foo = pin_init!(Foo {
//! #     a: new_mutex!(42, "Foo::a"),
//! #     b: 24,
//! # });
//! let foo: Result<Pin<Box<Foo>>> = Box::pin_init::<core::convert::Infallible>(foo);
//! ```
//!
//! ## Using a function/macro that returns an initializer
//!
//! Many types from the kernel supply a function/macro that returns an initializer, because the
//! above method only works for types where you can access the fields.
//!
//! ```rust
//! # use kernel::{new_mutex, sync::{Arc, Mutex}};
//! let mtx: Result<Arc<Mutex<usize>>> = Arc::pin_init(new_mutex!(42, "example::mtx"));
//! ```
//!
//! To declare an init macro/function you just return an `impl`[`PinInit<T, E>`]:
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! # use kernel::{sync::Mutex, prelude::*, new_mutex, init::PinInit};
//! #[pin_project]
//! struct DriverData {
//!     #[pin]
//!     status: Mutex<i32>,
//!     buffer: Box<[u8; 1_000_000]>,
//! }
//!
//! impl DriverData {
//!     fn new() -> impl PinInit<Self, Error> {
//!         pin_init!(Self {
//!             status: new_mutex!(0, "DriverData::status"),
//!             buffer: Box::init(kernel::init::zeroed())?,
//!         })
//!     }
//! }
//! ```
//!
//!
//! [^1]: That is not entirely true, only smart pointers that implement [`InPlaceInit`].
//!
//! [sync]: ../sync/index.html
//! [pinning]: https://doc.rust-lang.org/std/pin/index.html
//! [structurally pinned fields]: https://doc.rust-lang.org/std/pin/index.html#pinning-is-structural-for-field
//! [stack]: crate::stack_init
//! [`Arc<T>`]: crate::sync::Arc

use crate::{
    error::{self, Error},
    sync::UniqueArc,
};
use alloc::boxed::Box;
use core::{
    convert::Infallible,
    marker::{PhantomData, Unpin},
    mem::MaybeUninit,
    pin::Pin,
    ptr,
};

#[doc(hidden)]
pub mod __private;
mod pin_project;
mod pinned_drop;

/// Initialize a type directly on the stack.
///
/// # Examples
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, stack_init, init::*, macros::pin_project, sync::Mutex, new_mutex};
/// # use core::pin::Pin;
/// #[pin_project]
/// struct Foo {
///     #[pin]
///     a: Mutex<usize>,
///     b: Bar,
/// }
///
/// #[pin_project]
/// struct Bar {
///     x: u32,
/// }
///
/// let a = new_mutex!(42, "Foo::a");
///
/// stack_init!(let foo = pin_init!(Foo {
///     a,
///     b: Bar {
///         x: 64,
///     },
/// }));
/// let foo: Result<Pin<&mut Foo>> = foo;
/// ```
#[macro_export]
macro_rules! stack_init {
    (let $var:ident = $val:expr) => {
        let mut $var = $crate::init::__private::StackInit::uninit();
        let val = $val;
        let mut $var = unsafe { $crate::init::__private::StackInit::init(&mut $var, val) };
    };
    (let $var:ident $(: $t:ty)? =? $val:expr) => {
        let mut $var = $crate::init::__private::StackInit$(::<$t>)?::uninit();
        let val = $val;
        let mut $var = unsafe { $crate::init::__private::StackInit::init(&mut $var, val)? };
    };
}

/// Construct an in-place initializer for structs.
///
/// The syntax is identical to a normal struct initializer:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, macros::pin_project, init::*};
/// # use core::pin::Pin;
/// #[pin_project]
/// struct Foo {
///     a: usize,
///     b: Bar,
/// }
///
/// #[pin_project]
/// struct Bar {
///     x: u32,
/// }
///
/// # fn demo() -> impl PinInit<Foo> {
/// let a = 42;
///
/// let initializer = pin_init!(Foo {
///     a,
///     b: Bar {
///         x: 64,
///     },
/// });
/// # initializer }
/// # Box::pin_init(demo()).unwrap();
/// ```
/// Arbitrary rust expressions can be used to set the value of a variable.
///
/// # Init-functions
///
/// When working with this library it is often desired to let others construct your types without
/// giving access to all fields. This is where you would normally write a plain function `new`
/// that would return a new instance of your type. With this library that is also possible, however
/// there are a few extra things to keep in mind.
///
/// To create an initializer function, simple declare it like this:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, prelude::*, init::*};
/// # use core::pin::Pin;
/// # #[pin_project]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_project]
/// # struct Bar {
/// #     x: u32,
/// # }
///
/// impl Foo {
///     fn new() -> impl PinInit<Self> {
///         pin_init!(Self {
///             a: 42,
///             b: Bar {
///                 x: 64,
///             },
///         })
///     }
/// }
/// ```
///
/// Users of `Foo` can now create it like this:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, macros::pin_project, init::*};
/// # use core::pin::Pin;
/// # #[pin_project]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_project]
/// # struct Bar {
/// #     x: u32,
/// # }
/// # impl Foo {
/// #     fn new() -> impl PinInit<Self> {
/// #         pin_init!(Self {
/// #             a: 42,
/// #             b: Bar {
/// #                 x: 64,
/// #             },
/// #         })
/// #     }
/// # }
/// let foo = Box::pin_init(Foo::new());
/// ```
///
/// They can also easily embed it into their own `struct`s:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, macros::pin_project, init::*};
/// # use core::pin::Pin;
/// # #[pin_project]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_project]
/// # struct Bar {
/// #     x: u32,
/// # }
/// # impl Foo {
/// #     fn new() -> impl PinInit<Self> {
/// #         pin_init!(Self {
/// #             a: 42,
/// #             b: Bar {
/// #                 x: 64,
/// #             },
/// #         })
/// #     }
/// # }
/// #[pin_project]
/// struct FooContainer {
///     #[pin]
///     foo1: Foo,
///     #[pin]
///     foo2: Foo,
///     other: u32,
/// }
///
/// impl FooContainer {
///     fn new(other: u32) -> impl PinInit<Self> {
///         pin_init!(Self {
///             foo1: Foo::new(),
///             foo2: Foo::new(),
///             other,
///         })
///     }
/// }
/// ```
#[macro_export]
macro_rules! pin_init {
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($field:ident $(: $val:expr)?),*
        $(,)?
    }) => {
        $crate::pin_init!(@this($($this)?), @type_name($t $(<$($generics),*>)?), @typ($t $(<$($generics),*>)?), @fields($($field $(: $val)?),*))
    };
    (@this($($this:ident)?), @type_name($t:ident $(<$($generics:ty),*>)?), @typ($ty:ty), @fields($($field:ident $(: $val:expr)?),*)) => {{
        // we do not want to allow arbitrary returns
        struct __InitOk;
        let init = move |slot: *mut $ty| -> ::core::result::Result<__InitOk, _> {
            {
                // shadow the structure so it cannot be used to return early
                struct __InitOk;
                $(let $this = unsafe { ::core::ptr::NonNull::new_unchecked(slot) };)?
                $(
                    $(let $field = $val;)?
                    // call the initializer
                    // SAFETY: slot is valid, because we are inside of an initializer closure, we return
                    //         when an error/panic occurs.
                    unsafe {
                        <$ty as $crate::init::__private::__PinData>::__PinData::$field(
                            ::core::ptr::addr_of_mut!((*slot).$field),
                            $field,
                        )?;
                    }
                    // create the drop guard
                    // SAFETY: we forget the guard later when initialization has succeeded.
                    let $field = unsafe { $crate::init::__private::DropGuard::new(::core::ptr::addr_of_mut!((*slot).$field)) };
                    // only give access to &DropGuard, so it cannot be accidentally forgotten
                    let $field = &$field;
                )*
                #[allow(unreachable_code, clippy::diverging_sub_expression)]
                if false {
                    let _: $t $(<$($generics),*>)? = $t {
                        $($field: ::core::todo!()),*
                    };
                }
                $(
                    // forget each guard
                    unsafe { $crate::init::__private::DropGuard::forget($field) };
                )*
            }
            Ok(__InitOk)
        };
        let init = move |slot: *mut $ty| -> ::core::result::Result<(), _> {
            init(slot).map(|__InitOk| ())
        };
        let init = unsafe { $crate::init::pin_init_from_closure::<$t $(<$($generics),*>)?, _>(init) };
        init
    }}
}

/// Construct an in-place initializer for structs.
///
/// The syntax is identical to a normal struct initializer:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, init::*};
/// # use core::pin::Pin;
/// struct Foo {
///     a: usize,
///     b: Bar,
/// }
///
/// struct Bar {
///     x: u32,
/// }
///
/// # fn demo() -> impl Init<Foo> {
/// let a = 42;
///
/// let initializer = init!(Foo {
///     a,
///     b: Bar {
///         x: 64,
///     },
/// });
/// # initializer }
/// # Box::init(demo()).unwrap();
/// ```
///
/// Arbitrary rust expressions can be used to set the value of a variable.
///
/// # Init-functions
///
/// When working with this library it is often desired to let others construct your types without
/// giving access to all fields. This is where you would normally write a plain function `new`
/// that would return a new instance of your type. With this library that is also possible, however
/// there are a few extra things to keep in mind.
///
/// To create an initializer function, simple declare it like this:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, init::*};
/// # use core::pin::Pin;
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # struct Bar {
/// #     x: u32,
/// # }
///
/// impl Foo {
///     fn new() -> impl Init<Self> {
///         init!(Self {
///             a: 42,
///             b: Bar {
///                 x: 64,
///             },
///         })
///     }
/// }
/// ```
///
/// Users of `Foo` can now create it like this:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, init::*};
/// # use core::pin::Pin;
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # struct Bar {
/// #     x: u32,
/// # }
/// # impl Foo {
/// #     fn new() -> impl Init<Self> {
/// #         init!(Self {
/// #             a: 42,
/// #             b: Bar {
/// #                 x: 64,
/// #             },
/// #         })
/// #     }
/// # }
/// let foo = Box::init(Foo::new());
/// ```
///
/// They can also easily embed it into their own `struct`s:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, init::*};
/// # use core::pin::Pin;
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # struct Bar {
/// #     x: u32,
/// # }
/// # impl Foo {
/// #     fn new() -> impl Init<Self> {
/// #         init!(Self {
/// #             a: 42,
/// #             b: Bar {
/// #                 x: 64,
/// #             },
/// #         })
/// #     }
/// # }
/// struct FooContainer {
///     foo1: Foo,
///     foo2: Foo,
///     other: u32,
/// }
///
/// impl FooContainer {
///     fn new(other: u32) -> impl Init<Self> {
///         init!(Self {
///             foo1: Foo::new(),
///             foo2: Foo::new(),
///             other,
///         })
///     }
/// }
/// ```
#[macro_export]
macro_rules! init {
    ($t:ident $(<$($generics:ty),* $(,)?>)? {
        $($field:ident $(: $val:expr)?),*
        $(,)?
    }) => {{
        // we do not want to allow arbitrary returns
        struct __InitOk;
        let init = move |slot: *mut $t $(<$($generics),*>)?| -> ::core::result::Result<__InitOk, _> {
            {
                // shadow the structure so it cannot be used to return early
                struct __InitOk;
                $(
                    $(let $field = $val;)?
                    // call the initializer
                    // SAFETY: slot is valid, because we are inside of an initializer closure, we return
                    //         when an error/panic occurs.
                    unsafe { $crate::init::__private::__InitImpl::__init($field, ::core::ptr::addr_of_mut!((*slot).$field))? };
                    // create the drop guard
                    // SAFETY: we forget the guard later when initialization has succeeded.
                    let $field = unsafe { $crate::init::__private::DropGuard::new(::core::ptr::addr_of_mut!((*slot).$field)) };
                    // only give access to &DropGuard, so it cannot be accidentally forgotten
                    let $field = &$field;
                )*
                #[allow(unreachable_code, clippy::diverging_sub_expression)]
                if false {
                    let _: $t $(<$($generics),*>)? = $t {
                        $($field: ::core::todo!()),*
                    };
                }
                $(
                    // forget each guard
                    unsafe { $crate::init::__private::DropGuard::forget($field) };
                )*
            }
            Ok(__InitOk)
        };
        let init = move |slot: *mut $t $(<$($generics),*>)?| -> ::core::result::Result<(), _> {
            init(slot).map(|__InitOk| ())
        };
        let init = unsafe { $crate::init::init_from_closure::<$t $(<$($generics),*>)?, _>(init) };
        init
    }}
}

/// A pinned initializer for `T`.
///
/// To use this initializer, you will need a suitable memory location that can hold a `T`. This can
/// be [`Box<T>`], [`Arc<T>`], [`UniqueArc<T>`], or even the stack (see [`stack_init!`]). Use the
/// `pin_init` function of a smart pointer like [`Arc::pin_init`] on this.
///
/// Also see the [module description](self).
///
/// # Safety
///
/// When implementing this type you will need to take great care. Also there are probably very few
/// cases where a manual implementation is necessary. Use [`from_value`] and
/// [`pin_init_from_closure`] where possible.
///
/// The [`PinInit::__pinned_init`] function
/// - returns `Ok(())` iff it initialized every field of slot,
/// - returns `Err(err)` iff it encountered an error and then cleaned slot, this means:
///     - slot can be deallocated without UB ocurring,
///     - slot does not need to be dropped,
///     - slot is not partially initialized.
///
/// [`Arc<T>`]: crate::sync::Arc
/// [`Arc::pin_init`]: crate::sync::Arc::pin_init
#[must_use = "An initializer must be used in order to create its value."]
pub unsafe trait PinInit<T, E = Infallible>: Sized {
    /// Initializes `slot`.
    ///
    /// # Safety
    ///
    /// `slot` is a valid pointer to uninitialized memory.
    /// The caller does not touch `slot` when `Err` is returned, they are only permitted to
    /// deallocate.
    /// The slot will not move, i.e. it will be pinned.
    unsafe fn __pinned_init(self, slot: *mut T) -> Result<(), E>;
}

/// An initializer for `T`.
///
/// To use this initializer, you will need a suitable memory location that can hold a `T`. This can
/// be [`Box<T>`], [`Arc<T>`], [`UniqueArc<T>`], or even the stack (see [`stack_init!`]). Use the
/// `init` function of a smart pointer like [`Box::init`] on this. Because [`PinInit<T, E>`] is a
/// super trait, you can use every function that takes it as well.
///
/// Also see the [module description](self).
///
/// # Safety
///
/// When implementing this type you will need to take great care. Also there are probably very few
/// cases where a manual implementation is necessary. Use [`from_value`] and
/// [`init_from_closure`] where possible.
///
/// The [`Init::__init`] function
/// - returns `Ok(())` iff it initialized every field of slot,
/// - returns `Err(err)` iff it encountered an error and then cleaned slot, this means:
///     - slot can be deallocated without UB ocurring,
///     - slot does not need to be dropped,
///     - slot is not partially initialized.
///
/// The `__pinned_init` function from the supertrait [`PinInit`] needs to exectute the exact same
/// code as `__init`.
///
/// Contrary to its supertype [`PinInit<T, E>`] the caller is allowed to
/// move the pointee after initialization.
///
/// [`Arc<T>`]: crate::sync::Arc
#[must_use = "An initializer must be used in order to create its value."]
pub unsafe trait Init<T, E = Infallible>: PinInit<T, E> {
    /// Initializes `slot`.
    ///
    /// # Safety
    ///
    /// `slot` is a valid pointer to uninitialized memory.
    /// The caller does not touch `slot` when `Err` is returned, they are only permitted to
    /// deallocate.
    unsafe fn __init(self, slot: *mut T) -> Result<(), E>;
}

type Invariant<T> = PhantomData<fn(T) -> T>;

struct InitClosure<F, T, E>(F, Invariant<(T, E)>);

unsafe impl<T, F, E> PinInit<T, E> for InitClosure<F, T, E>
where
    F: FnOnce(*mut T) -> Result<(), E>,
{
    #[inline]
    unsafe fn __pinned_init(self, slot: *mut T) -> Result<(), E> {
        (self.0)(slot)
    }
}

unsafe impl<T, F, E> Init<T, E> for InitClosure<F, T, E>
where
    F: FnOnce(*mut T) -> Result<(), E>,
{
    #[inline]
    unsafe fn __init(self, slot: *mut T) -> Result<(), E> {
        (self.0)(slot)
    }
}

/// Creates a new [`Init<T, E>`] from the given closure.
///
/// # Safety
///
/// The closure
/// - returns `Ok(())` iff it initialized every field of slot,
/// - returns `Err(err)` iff it encountered an error and then cleaned slot, this means:
///     - slot can be deallocated without UB ocurring,
///     - slot does not need to be dropped,
///     - slot is not partially initialized.
/// - slot may move after initialization
#[inline]
pub const unsafe fn init_from_closure<T, E>(
    f: impl FnOnce(*mut T) -> Result<(), E>,
) -> impl Init<T, E> {
    InitClosure(f, PhantomData)
}

/// Creates a new [`PinInit<T, E>`] from the given closure.
///
/// # Safety
///
/// The closure
/// - returns `Ok(())` iff it initialized every field of slot,
/// - returns `Err(err)` iff it encountered an error and then cleaned slot, this means:
///     - slot can be deallocated without UB ocurring,
///     - slot does not need to be dropped,
///     - slot is not partially initialized.
/// - may assume that the slot does not move if `T: !Unpin`
#[inline]
pub const unsafe fn pin_init_from_closure<T, E>(
    f: impl FnOnce(*mut T) -> Result<(), E>,
) -> impl PinInit<T, E> {
    InitClosure(f, PhantomData)
}

/// Trait facilitating pinned destruction.
///
/// Use [`pinned_drop`] to implement this trait safely:
/// ```rust
/// # use kernel::sync::Mutex;
/// use kernel::macros::pinned_drop;
/// #[pin_project(PinnedDrop)]
/// struct Foo {
///     #[pin]
///     mtx: Mutex<usize>,
/// }
///
/// #[pinned_drop]
/// impl PinnedDrop for Foo {
///     fn drop(self: Pin<&mut Self>) {
///         pr_info!("Foo is being dropped!");
///     }
/// }
/// ```
///
/// # Safety
///
/// This trait must be implemented with [`pinned_drop`].
///
/// [`pinned_drop`]: kernel::macros::pinned_drop
pub unsafe trait PinnedDrop {
    /// Executes the pinned destructor of this type.
    ///
    /// # Safety
    ///
    /// Only call this from `<Self as Drop>::drop`.
    unsafe fn drop(self: Pin<&mut Self>);

    // used by `pinned_drop` to ensure that only safe operations are used in `drop`.
    #[doc(hidden)]
    fn __ensure_no_unsafe_op_in_drop(self: Pin<&mut Self>);
}

/// Smart pointer that can initialize memory in-place.
pub trait InPlaceInit<T>: Sized {
    /// Use the given initializer to in-place initialize a `T`.
    ///
    /// If `T: !Unpin` it will not be able to move afterwards.
    fn pin_init<E>(init: impl PinInit<T, E>) -> error::Result<Pin<Self>>
    where
        Error: From<E>;

    /// Use the given initializer to in-place initialize a `T`.
    fn init<E>(init: impl Init<T, E>) -> error::Result<Self>
    where
        Error: From<E>;
}

impl<T> InPlaceInit<T> for Box<T> {
    #[inline]
    fn pin_init<E>(init: impl PinInit<T, E>) -> error::Result<Pin<Self>>
    where
        Error: From<E>,
    {
        let mut this = Box::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: when init errors/panics, slot will get deallocated but not dropped,
        // slot is valid and will not be moved because of the `Pin::new_unchecked`
        unsafe { init.__pinned_init(slot)? };
        // SAFETY: all fields have been initialized
        Ok(unsafe { Pin::new_unchecked(this.assume_init()) })
    }

    #[inline]
    fn init<E>(init: impl Init<T, E>) -> error::Result<Self>
    where
        Error: From<E>,
    {
        let mut this = Box::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: when init errors/panics, slot will get deallocated but not dropped,
        // slot is valid
        unsafe { init.__init(slot)? };
        // SAFETY: all fields have been initialized
        Ok(unsafe { this.assume_init() })
    }
}

impl<T> InPlaceInit<T> for UniqueArc<T> {
    #[inline]
    fn pin_init<E>(init: impl PinInit<T, E>) -> error::Result<Pin<Self>>
    where
        Error: From<E>,
    {
        let mut this = UniqueArc::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: when init errors/panics, slot will get deallocated but not dropped,
        // slot is valid and will not be moved because of the `Pin::new_unchecked`
        unsafe { init.__pinned_init(slot)? };
        // SAFETY: all fields have been initialized
        Ok(unsafe { Pin::new_unchecked(this.assume_init()) })
    }

    #[inline]
    fn init<E>(init: impl Init<T, E>) -> error::Result<Self>
    where
        Error: From<E>,
    {
        let mut this = UniqueArc::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: when init errors/panics, slot will get deallocated but not dropped,
        // slot is valid
        unsafe { init.__init(slot)? };
        // SAFETY: all fields have been initialized
        Ok(unsafe { this.assume_init() })
    }
}

/// Marker trait for types that can be initialized by writing just zeroes.
///
/// # Safety
///
/// The bit pattern consisting of only zeroes must be a valid bit pattern for the type.
pub unsafe trait Zeroable {}

/// Create a new zeroed T.
///
/// The returned initializer will write `0x00` to every byte of the given slot.
#[inline]
pub fn zeroed<T: Zeroable + Unpin>() -> impl Init<T> {
    // SAFETY: because `T: Zeroable`, all bytes zero is a valid bit pattern for `T`
    //         and because we write all zeroes, the memory is initialized.
    unsafe {
        init_from_closure(|slot: *mut T| {
            slot.write_bytes(0, 1);
            Ok(())
        })
    }
}

/// An initializer that leaves the memory uninitialized.
///
/// The initializer is a no-op. The slot memory is not changed.
#[inline]
pub fn uninit<T>() -> impl Init<MaybeUninit<T>> {
    // SAFETY: The memory is allowed to be uninitialized.
    unsafe { init_from_closure(|_| Ok(())) }
}

/// Convert a value into an initializer.
///
/// Directly moves the value into the given slot.
#[inline]
pub fn from_value<T>(value: T) -> impl Init<T> {
    // SAFETY: we use the value to initialize the slot.
    unsafe {
        init_from_closure(move |slot: *mut T| {
            slot.write(value);
            Ok(())
        })
    }
}

macro_rules! impl_zeroable {
    ($($t:ty),*) => {
        $(unsafe impl Zeroable for $t {})*
    };
}
// All primitives that are allowed to be zero.
impl_zeroable!(
    bool, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);
// There is nothing to zero.
impl_zeroable!(core::marker::PhantomPinned, Infallible, ());

// We are allowed to zero padding bytes.
unsafe impl<const N: usize, T: Zeroable> Zeroable for [T; N] {}

// There is nothing to zero.
unsafe impl<T: ?Sized> Zeroable for PhantomData<T> {}

// `null` pointer is valid.
unsafe impl<T: ?Sized> Zeroable for *mut T {}
unsafe impl<T: ?Sized> Zeroable for *const T {}

macro_rules! impl_tuple_zeroable {
    ($(,)?) => {};
    ($first:ident, $($t:ident),* $(,)?) => {
        // all elements are zeroable and padding can be zero
        unsafe impl<$first: Zeroable, $($t: Zeroable),*> Zeroable for ($first, $($t),*) {}
        impl_tuple_zeroable!($($t),* ,);
    }
}

impl_tuple_zeroable!(A, B, C, D, E, F, G, H, I, J);
