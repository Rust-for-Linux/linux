// SPDX-License-Identifier: Apache-2.0 OR MIT

//! API to safely and fallibly initialize pinned `struct`s using in-place constructors.
//!
//! It also allows in-place initialization of big `struct`s that would otherwise produce a stack
//! overflow.
//!
//! Most `struct`s from the [`sync`] module need to be pinned, because they contain self-referential
//! `struct`s from C. [Pinning][pinning] is Rust's way of ensuring data does not move.
//!
//! # Overview
//!
//! To initialize a `struct` with an in-place constructor you will need two things:
//! - an in-place constructor,
//! - a memory location that can hold your `struct` (this can be the [stack], an [`Arc<T>`],
//!   [`UniqueArc<T>`], [`Box<T>`] or any other smart pointer that implements [`InPlaceInit`]).
//!
//! To get an in-place constructor there are generally three options:
//! - directly creating an in-place constructor using the [`pin_init!`] macro,
//! - a custom function/macro returning an in-place constructor provided by someone else,
//! - using the unsafe function [`pin_init_from_closure()`] to manually create an initializer.
//!
//! Aside from pinned initialization, this API also supports in-place construction without pinning,
//! the marcos/types/functions are generally named like the pinned variants without the `pin`
//! prefix.
//!
//! # Examples
//!
//! ## Using the [`pin_init!`] macro
//!
//! If you want to use [`PinInit`], then you will have to annotate your `struct` with
//! `#[`[`pin_data`]`]`. It is a macro that uses `#[pin]` as a marker for
//! [structurally pinned fields]. After doing this, you can then create an in-place constructor via
//! [`pin_init!`]. The syntax is almost the same as normal `struct` initializers. The difference is
//! that you need to write `<-` instead of `:` for fields that you want to initialize in-place.
//!
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! use kernel::{prelude::*, sync::Mutex, new_mutex};
//! # use core::pin::Pin;
//! #[pin_data]
//! struct Foo {
//!     #[pin]
//!     a: Mutex<usize>,
//!     b: u32,
//! }
//!
//! let foo = pin_init!(Foo {
//!     a <- new_mutex!(42, "Foo::a"),
//!     b: 24,
//! });
//! ```
//!
//! `foo` now is of the type [`impl PinInit<Foo>`]. We can now use any smart pointer that we like
//! (or just the stack) to actually initialize a `Foo`:
//!
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! # use kernel::{prelude::*, sync::Mutex, new_mutex};
//! # use core::pin::Pin;
//! # #[pin_data]
//! # struct Foo {
//! #     #[pin]
//! #     a: Mutex<usize>,
//! #     b: u32,
//! # }
//! # let foo = pin_init!(Foo {
//! #     a <- new_mutex!(42, "Foo::a"),
//! #     b: 24,
//! # });
//! let foo: Result<Pin<Box<Foo>>> = Box::pin_init(foo);
//! ```
//!
//! For more information see the [`pin_init!`] macro.
//!
//! ## Using a custom function/macro that returns an initializer
//!
//! Many types from the kernel supply a function/macro that returns an initializer, because the
//! above method only works for types where you can access the fields.
//!
//! ```rust
//! # use kernel::{new_mutex, sync::{Arc, Mutex}};
//! let mtx: Result<Arc<Mutex<usize>>> = Arc::pin_init(new_mutex!(42, "example::mtx"));
//! ```
//!
//! To declare an init macro/function you just return an [`impl PinInit<T, E>`]:
//!
//! ```rust
//! # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
//! # use kernel::{sync::Mutex, prelude::*, new_mutex, init::PinInit, try_pin_init};
//! #[pin_data]
//! struct DriverData {
//!     #[pin]
//!     status: Mutex<i32>,
//!     buffer: Box<[u8; 1_000_000]>,
//! }
//!
//! impl DriverData {
//!     fn new() -> impl PinInit<Self, Error> {
//!         try_pin_init!(Self {
//!             status <- new_mutex!(0, "DriverData::status"),
//!             buffer: Box::init(kernel::init::zeroed())?,
//!         })
//!     }
//! }
//! ```
//!
//! ## Manual creation of an initializer
//!
//! Often when working with primitives the previous approaches are not sufficient. That is where
//! [`pin_init_from_closure()`] comes in. This `unsafe` function allows you to create a
//! [`impl PinInit<T, E>`] directly from a closure. Of course you have to ensure that the closure
//! actually does the initialization in the correct way. Here are the things to look out for
//! (we are calling the parameter to the closure `slot`):
//! - when the closure returns `Ok(())`, then it has completed the initialization successfully, so
//!   `slot` now contains a valid bit pattern for the type `T`,
//! - when the closure returns `Err(e)`, then the caller may deallocate the memory at `slot`, so
//!   you need to take care to clean up anything if your initialization fails mid-way,
//! - you may assume that `slot` will stay pinned even after the closure returns until `drop` of
//!   `slot` gets called.
//!
//! ```rust
//! use kernel::init::{PinInit, self};
//! use core::{ptr::addr_of_mut, marker::PhantomPinned};
//! # mod bindings {
//! #     pub struct foo;
//! #     pub unsafe fn init_foo(_ptr: *mut foo) {}
//! #     pub unsafe fn destroy_foo(_ptr: *mut foo) {}
//! #     pub unsafe fn enable_foo(_ptr: *mut foo, _flags: u32) -> i32 { 0 }
//! # }
//! /// # Invariants
//! ///
//! /// `foo` is always initialized
//! pub struct RawFoo {
//!     foo: Opaque<bindings::foo>,
//!     _p: PhantomPinned,
//! }
//!
//! impl RawFoo {
//!     pub fn new(flags: u32) -> impl PinInit<Self, Error> {
//!         // SAFETY:
//!         // - when the closure returns `Ok(())`, then it has successfully initialized and
//!         //   enabled `foo`,
//!         // - when it returns `Err(e)`, then it has cleaned up before
//!         unsafe {
//!             init::pin_init_from_closure(move |slot: *mut Self| {
//!                 // `slot` contains uninit memory, avoid creating a reference.
//!                 let foo = addr_of_mut!((*slot).foo);
//!
//!                 // Initialize the `foo`
//!                 bindings::init_foo(Opaque::raw_get(foo));
//!
//!                 // Try to enable it.
//!                 let err = bindings::enable_foo(Opaque::raw_get(foo), flags);
//!                 if err != 0 {
//!                     // Enabling has failed, first clean up the foo and then return the error.
//!                     bindings::destroy_foo(Opaque::raw_get(foo));
//!                     return Err(Error::from_kernel_errno(err));
//!                 }
//!
//!                 // All fields of `RawFoo` have been initialized, since `_p` is a ZST.
//!                 Ok(())
//!             })
//!         }
//!     }
//! }
//!
//! impl Drop for RawFoo {
//!     fn drop(&mut self) {
//!         // SAFETY: Since `foo` is initialized, destroying is safe.
//!         unsafe { bindings::destroy_foo(self.foo.get()) };
//!     }
//! }
//! ```
//!
//! For the special case where initializing a field is a single FFI-function call that cannot fail,
//! there exist helper functions in [`kernel::init::common`]. These functions initialize a single
//! [`Opaque`] field by just delegating to the FFI-function. You can use these in combination with
//! [`pin_init!`].
//!
//! For more information on how to use [`pin_init_from_closure()`], take a look at the uses inside
//! the `kernel` crate. The [`sync`] module is a good starting point.
//!
//! [`sync`]: kernel::sync
//! [pinning]: https://doc.rust-lang.org/std/pin/index.html
//! [structurally pinned fields]:
//!     https://doc.rust-lang.org/std/pin/index.html#pinning-is-structural-for-field
//! [stack]: crate::stack_pin_init
//! [`Arc<T>`]: crate::sync::Arc
//! [`impl PinInit<Foo>`]: PinInit
//! [`impl PinInit<T, E>`]: PinInit
//! [`impl Init<T, E>`]: Init
//! [`Opaque`]: kernel::types::Opaque
//! [`pin_data`]: ::macros::pin_data

use crate::{
    error::{self, Error},
    sync::UniqueArc,
};
use alloc::boxed::Box;
use core::{
    cell::Cell,
    convert::Infallible,
    marker::{PhantomData, Unpin},
    mem::MaybeUninit,
    pin::Pin,
    ptr,
};

pub mod common;
#[doc(hidden)]
pub mod macros;

/// Initialize and pin a type directly on the stack.
///
/// # Examples
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, stack_pin_init, init::*, sync::Mutex, new_mutex};
/// # use macros::pin_data;
/// # use core::pin::Pin;
/// #[pin_data]
/// struct Foo {
///     #[pin]
///     a: Mutex<usize>,
///     b: Bar,
/// }
///
/// #[pin_data]
/// struct Bar {
///     x: u32,
/// }
///
/// let a = new_mutex!(42, "Foo::a");
///
/// stack_pin_init!(let foo =? pin_init!(Foo {
///     a,
///     b: Bar {
///         x: 64,
///     },
/// }));
/// let foo: Pin<&mut Foo> = foo;
/// # Ok::<(), core::convert::Infallible>(())
/// ```
///
/// # Syntax
///
/// A normal `let` binding with optional type annotation. The expression is expected to implement
/// [`PinInit`]. Additionally a `?` can be put after the `=`, this will assign `Pin<&mut T>` to the
/// variable instead of `Result<Pin<&mut T>, E>`.
#[macro_export]
macro_rules! stack_pin_init {
    (let $var:ident $(: $t:ty)? = $val:expr) => {
        let mut $var = $crate::init::StackInit$(::<$t>)?::uninit();
        let mut $var = {
            let val = $val;
            unsafe { $crate::init::StackInit::init(&mut $var, val) }
        };
    };
    (let $var:ident $(: $t:ty)? =? $val:expr) => {
        let mut $var = $crate::init::StackInit$(::<$t>)?::uninit();
        let mut $var = {
            let val = $val;
            unsafe { $crate::init::StackInit::init(&mut $var, val)? }
        };
    };
}

/// Construct an in-place, pinned initializer for `struct`s.
///
/// This macro defaults the error to [`Infallible`]. If you need [`Error`], then use
/// [`try_pin_init!`].
///
/// The syntax is almost identical to that of a normal `struct` initializer:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, macros::pin_data, init::*};
/// # use core::pin::Pin;
/// #[pin_data]
/// struct Foo {
///     a: usize,
///     b: Bar,
/// }
///
/// #[pin_data]
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
///
/// Arbitrary Rust expressions can be used to set the value of a variable.
///
/// # Init-functions
///
/// When working with this API it is often desired to let others construct your types without
/// giving access to all fields. This is where you would normally write a plain function `new`
/// that would return a new instance of your type. With this API that is also possible.
/// However, there are a few extra things to keep in mind.
///
/// To create an initializer function, simply declare it like this:
///
/// ```rust
/// # #![allow(clippy::disallowed_names, clippy::new_ret_no_self)]
/// # use kernel::{init, pin_init, prelude::*, init::*};
/// # use core::pin::Pin;
/// # #[pin_data]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_data]
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
/// # use kernel::{init, pin_init, macros::pin_data, init::*};
/// # use core::pin::Pin;
/// # #[pin_data]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_data]
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
/// # use kernel::{init, pin_init, macros::pin_data, init::*};
/// # use core::pin::Pin;
/// # #[pin_data]
/// # struct Foo {
/// #     a: usize,
/// #     b: Bar,
/// # }
/// # #[pin_data]
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
/// #[pin_data]
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
///             foo1 <- Foo::new(),
///             foo2 <- Foo::new(),
///             other,
///         })
///     }
/// }
/// ```
///
/// Here we see that when using `pin_init!` with `PinInit`, one needs to write `<-` instead of `:`.
/// This signifies that the given field is initialized in-place. As with `struct` initializers, just
/// writing the field (in this case `other`) without `:` or `<-` means `other: other,`.
///
/// # Syntax
///
/// As already mentioned in the examples above, inside of `pin_init!` a `struct` initializer with
/// the following modifications is expected:
/// - Fields that you want to initialize in-place have to use `<-` instead of `:`.
/// - In front of the initializer you can write `&this in` to have access to a [`NonNull<Self>`]
///   pointer named `this` inside of the initializer.
///
/// For instance:
///
/// ```rust
/// # use kernel::pin_init;
/// # use macros::pin_data;
/// # use core::{ptr::addr_of_mut, marker::PhantomPinned};
/// #[pin_data]
/// struct Buf {
///     ptr: *mut u8,
///     buf: [u8; 64],
///     #[pin]
///     pin: PhantomPinned,
/// }
/// pin_init!(&this in Buf {
///     buf: [0; 64],
///     ptr: unsafe { addr_of_mut!((*this.as_ptr()).buf).cast() },
///     pin: PhantomPinned,
/// });
/// ```
///
/// [`try_pin_init!`]: kernel::try_pin_init
/// [`NonNull<Self>`]: core::ptr::NonNull
#[macro_export]
macro_rules! pin_init {
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }) => {
        $crate::try_pin_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error(::core::convert::Infallible),
        )
    };
}

/// Construct an in-place, fallible pinned initializer for `struct`s.
///
/// If the initialization can complete without error (or [`Infallible`]), then use [`pin_init!`].
///
/// You can use the `?` operator or use `return Err(err)` inside the initializer to stop
/// initialization and return the error.
///
/// IMPORTANT: if you have `unsafe` code inside of the initializer you have to ensure that when
/// initialization fails, the memory can be safely deallocated without any further modifications.
///
/// This macro defaults the error to [`Error`].
///
/// The syntax is identical to [`pin_init!`] with the following exception: you can append `? $type`
/// after the `struct` initializer to specify the error type you want to use.
///
/// # Examples
///
/// ```rust
/// use kernel::{init::PinInit, error::Error};
/// struct BigBuf {
///     big: Box<[u8; 1024 * 1024 * 1024]>,
///     small: [u8; 1024 * 1024],
///     ptr: *mut u8,
/// }
///
/// impl BigBuf {
///     fn new() -> impl PinInit<Self, Error> {
///         try_pin_init!(Self {
///             big: {
///                 let zero = Box::try_new_zeroed()?;
///                 unsafe { zero.assume_init() }
///             },
///             small: [0; 1024 * 1024],
///             ptr: core::ptr::null_mut(),
///         }? Error)
///     }
/// }
/// ```
#[macro_export]
macro_rules! try_pin_init {
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }) => {
        $crate::try_pin_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error($crate::error::Error),
        )
    };
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }? $err:ty) => {
        $crate::try_pin_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error($err),
        )
    };
    (
        @this($($this:ident)?),
        @type_name($t:ident),
        @typ($ty:ty),
        @fields($($fields:tt)*),
        @error($err:ty),
    ) => {{
        // We do not want to allow arbitrary returns, so we declare this type as the `Ok` return
        // type and shadow it later when we insert the arbitrary user code. That way there will be
        // no possibility of returning without `unsafe`.
        struct __InitOk;
        let init = move |slot: *mut $ty| -> ::core::result::Result<__InitOk, $err> {
            {
                // Shadow the structure so it cannot be used to return early.
                struct __InitOk;
                // Create the `this` so it can be referenced by the user inside of the expressions
                // creating the individual fields.
                $(let $this = unsafe { ::core::ptr::NonNull::new_unchecked(slot) };)?
                // Initialize every field.
                $crate::try_pin_init!(init_slot:
                    @typ($ty),
                    @slot(slot),
                    @munch_fields($($fields)*,),
                );
                // We use unreachable code to ensure that all fields have been mentioned exactly
                // once, this struct initializer will still be type-checked and complain with a
                // very natural error message if a field is forgotten/mentioned more than once.
                #[allow(unreachable_code, clippy::diverging_sub_expression)]
                if false {
                    $crate::try_pin_init!(make_initializer:
                        @typ($ty),
                        @type_name($t),
                        @munch_fields($($fields)*,),
                        @acc(),
                    );
                }
                // Forget all guards, since initialization was a success.
                $crate::try_pin_init!(forget_guards:
                    @munch_fields($($fields)*,),
                );
            }
            Ok(__InitOk)
        };
        let init = move |slot: *mut $ty| -> ::core::result::Result<(), $err> {
            init(slot).map(|__InitOk| ())
        };
        let init = unsafe { $crate::init::pin_init_from_closure::<$ty, $err>(init) };
        init
    }};
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        @munch_fields($(,)?),
    ) => {
        // Endpoint of munching, no fields are left.
    };
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        // In-place initialization syntax.
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
    ) => {
        let $field = $val;
        // Call the initializer.
        //
        // SAFETY: `slot` is valid, because we are inside of an initializer closure, we
        // return when an error/panic occurs.
        // We also use the `__PinData` to request the correct trait (`Init` or `PinInit`).
        unsafe {
            <$ty as $crate::init::__PinData>::__PinData::$field(
                ::core::ptr::addr_of_mut!((*$slot).$field),
                $field,
            )?;
        }
        // Create the drop guard.
        //
        // SAFETY: We forget the guard later when initialization has succeeded.
        let $field = unsafe {
            $crate::init::DropGuard::new(::core::ptr::addr_of_mut!((*$slot).$field))
        };
        // Only give access to `&DropGuard`, so it cannot be forgotten via safe code.
        let $field = &$field;

        $crate::try_pin_init!(init_slot:
            @typ($ty),
            @slot($slot),
            @munch_fields($($rest)*),
        );
    };
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        // Direct value init, this is safe for every field.
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
    ) => {
        $(let $field = $val;)?
        // Call the initializer.
        //
        // SAFETY: The memory at `slot` is uninitialized.
        unsafe { ::core::ptr::write(::core::ptr::addr_of_mut!((*$slot).$field), $field) };
        // Create the drop guard:
        //
        // SAFETY: We forget the guard later when initialization has succeeded.
        let $field = unsafe {
            $crate::init::DropGuard::new(::core::ptr::addr_of_mut!((*$slot).$field))
        };
        // Only give access to `&DropGuard`, so it cannot be accidentally forgotten.
        let $field = &$field;

        $crate::try_pin_init!(init_slot:
            @typ($ty),
            @slot($slot),
            @munch_fields($($rest)*),
        );
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields($(,)?),
        @acc($($acc:tt)*),
    ) => {
        // Endpoint, nothing more to munch.
        let _: $ty = $t {
            $($acc)*
        };
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
        @acc($($acc:tt)*),
    ) => {
        $crate::try_pin_init!(make_initializer:
            @typ($ty),
            @type_name($t),
            @munch_fields($($rest)*),
            @acc($($acc)* $field: ::core::panic!(),),
        );
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
        @acc($($acc:tt)*),
    ) => {
        $crate::try_pin_init!(make_initializer:
            @typ($ty),
            @type_name($t),
            @munch_fields($($rest)*),
            @acc($($acc)* $field: ::core::panic!(),),
        );
    };
    (forget_guards:
        @munch_fields($(,)?),
    ) => {
        // Munching finished.
    };
    (forget_guards:
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
    ) => {
        unsafe { $crate::init::DropGuard::forget($field) };

        $crate::try_pin_init!(forget_guards:
            @munch_fields($($rest)*),
        );
    };
    (forget_guards:
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
    ) => {
        unsafe { $crate::init::DropGuard::forget($field) };

        $crate::try_pin_init!(forget_guards:
            @munch_fields($($rest)*),
        );
    };
}

/// Construct an in-place initializer for `struct`s.
///
/// This macro defaults the error to [`Infallible`]. If you need [`Error`], then use
/// [`try_init!`].
///
/// The syntax is identical to [`pin_init!`].
///
/// This initializer is for initializing data in-place that might later be moved. If you want to
/// pin-initialize, use [`pin_init!`].
#[macro_export]
macro_rules! init {
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }) => {
        $crate::try_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error(::core::convert::Infallible),
        )
    }
}

/// Construct an in-place fallible initializer for `struct`s.
///
/// This macro defaults the error to [`Error`]. If you need [`Infallible`], then use
/// [`init!`].
///
/// The syntax is identical to [`try_pin_init!`]. If you want to specify a custom error,
/// append `? $type` after the `struct` initializer.
///
/// # Examples
///
/// ```rust
/// use kernel::{init::PinInit, error::Error};
/// struct BigBuf {
///     big: Box<[u8; 1024 * 1024 * 1024]>,
///     small: [u8; 1024 * 1024],
/// }
///
/// impl BigBuf {
///     fn new() -> impl Init<Self, Error> {
///         try_init!(Self {
///             big: {
///                 let zero = Box::try_new_zeroed()?;
///                 unsafe { zero.assume_init() }
///             },
///             small: [0; 1024 * 1024],
///         }? Error)
///     }
/// }
/// ```
#[macro_export]
macro_rules! try_init {
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }) => {
        $crate::try_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error($crate::error::Error),
        )
    };
    ($(&$this:ident in)? $t:ident $(<$($generics:ty),* $(,)?>)? {
        $($fields:tt)*
    }? $err:ty) => {
        $crate::try_init!(
            @this($($this)?),
            @type_name($t),
            @typ($t $(<$($generics),*>)?),
            @fields($($fields)*),
            @error($err),
        )
    };
    (
        @this($($this:ident)?),
        @type_name($t:ident),
        @typ($ty:ty),
        @fields($($fields:tt)*),
        @error($err:ty),
    ) => {{
        // We do not want to allow arbitrary returns, so we declare this type as the `Ok` return
        // type and shadow it later when we insert the arbitrary user code. That way there will be
        // no possibility of returning without `unsafe`.
        struct __InitOk;
        let init = move |slot: *mut $ty| -> ::core::result::Result<__InitOk, $err> {
            {
                // Shadow the structure so it cannot be used to return early.
                struct __InitOk;
                // Create the `this` so it can be referenced by the user inside of the expressions
                // creating the individual fields.
                $(let $this = unsafe { ::core::ptr::NonNull::new_unchecked(slot) };)?
                // Initialize every field.
                $crate::try_init!(init_slot:
                    @typ($ty),
                    @slot(slot),
                    @munch_fields($($fields)*,),
                );
                // We use unreachable code to ensure that all fields have been mentioned exactly
                // once, this struct initializer will still be type-checked and complain with a
                // very natural error message if a field is forgotten/mentioned more than once.
                #[allow(unreachable_code, clippy::diverging_sub_expression)]
                if false {
                    $crate::try_init!(make_initializer:
                        @typ($ty),
                        @type_name($t),
                        @munch_fields($($fields)*,),
                        @acc(),
                    );
                }
                // Forget all guards, since initialization was a success.
                $crate::try_init!(forget_guards:
                    @munch_fields($($fields)*,),
                );
            }
            Ok(__InitOk)
        };
        let init = move |slot: *mut $ty| -> ::core::result::Result<(), $err> {
            init(slot).map(|__InitOk| ())
        };
        let init = unsafe { $crate::init::init_from_closure::<$ty, $err>(init) };
        init
    }};
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        @munch_fields( $(,)?),
    ) => {
        // Endpoint of munching, no fields are left.
    };
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
    ) => {
        let $field = $val;
        // Call the initializer.
        //
        // SAFETY: `slot` is valid, because we are inside of an initializer closure, we
        // return when an error/panic occurs.
        unsafe {
            $crate::init::Init::__init($field, ::core::ptr::addr_of_mut!((*$slot).$field))?;
        }
        // Create the drop guard.
        //
        // SAFETY: We forget the guard later when initialization has succeeded.
        let $field = unsafe {
            $crate::init::DropGuard::new(::core::ptr::addr_of_mut!((*$slot).$field))
        };
        // Only give access to `&DropGuard`, so it cannot be accidentally forgotten.
        let $field = &$field;

        $crate::try_init!(init_slot:
            @typ($ty),
            @slot($slot),
            @munch_fields($($rest)*),
        );
    };
    (init_slot:
        @typ($ty:ty),
        @slot($slot:ident),
        // Direct value init.
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
    ) => {
        $(let $field = $val;)?
        // Call the initializer.
        //
        // SAFETY: The memory at `slot` is uninitialized.
        unsafe { ::core::ptr::write(::core::ptr::addr_of_mut!((*$slot).$field), $field) };
        // Create the drop guard.
        //
        // SAFETY: We forget the guard later when initialization has succeeded.
        let $field = unsafe {
            $crate::init::DropGuard::new(::core::ptr::addr_of_mut!((*$slot).$field))
        };
        // Only give access to `&DropGuard`, so it cannot be accidentally forgotten.
        let $field = &$field;

        $crate::try_init!(init_slot:
            @typ($ty),
            @slot($slot),
            @munch_fields($($rest)*),
        );
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields( $(,)?),
        @acc($($acc:tt)*),
    ) => {
        // Endpoint, nothing more to munch.
        let _: $ty = $t {
            $($acc)*
        };
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
        @acc($($acc:tt)*),
    ) => {
        $crate::try_init!(make_initializer:
            @typ($ty),
            @type_name($t),
            @munch_fields($($rest)*),
            @acc($($acc)*$field: ::core::panic!(),),
        );
    };
    (make_initializer:
        @typ($ty:ty),
        @type_name($t:ident),
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
        @acc($($acc:tt)*),
    ) => {
        $crate::try_init!(make_initializer:
            @typ($ty),
            @type_name($t),
            @munch_fields($($rest)*),
            @acc($($acc)*$field: ::core::panic!(),),
        );
    };
    (forget_guards:
        @munch_fields($(,)?),
    ) => {
        // Munching finished.
    };
    (forget_guards:
        @munch_fields($field:ident <- $val:expr, $($rest:tt)*),
    ) => {
        unsafe { $crate::init::DropGuard::forget($field) };

        $crate::try_init!(forget_guards:
            @munch_fields($($rest)*),
        );
    };
    (forget_guards:
        @munch_fields($field:ident $(: $val:expr)?, $($rest:tt)*),
    ) => {
        unsafe { $crate::init::DropGuard::forget($field) };

        $crate::try_init!(forget_guards:
            @munch_fields($($rest)*),
        );
    };
}

/// A pinned initializer for `T`.
///
/// To use this initializer, you will need a suitable memory location that can hold a `T`. This can
/// be [`Box<T>`], [`Arc<T>`], [`UniqueArc<T>`] or even the stack (see [`stack_pin_init!`]). Use the
/// [`InPlaceInit::pin_init`] function of a smart pointer like [`Arc::pin_init`] on this.
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
/// - returns `Ok(())` if it initialized every field of `slot`,
/// - returns `Err(err)` if it encountered an error and then cleaned `slot`, this means:
///     - `slot` can be deallocated without UB occurring,
///     - `slot` does not need to be dropped,
///     - `slot` is not partially initialized.
/// - while constructing the `T` at `slot` it upholds the pinning invariants of `T`.
///
/// [`Arc<T>`]: crate::sync::Arc
/// [`Arc::pin_init`]: crate::sync::Arc::pin_init
#[must_use = "An initializer must be used in order to create its value."]
pub unsafe trait PinInit<T: ?Sized, E = Infallible>: Sized {
    /// Initializes `slot`.
    ///
    /// # Safety
    ///
    /// - `slot` is a valid pointer to uninitialized memory.
    /// - the caller does not touch `slot` when `Err` is returned, they are only permitted to
    ///   deallocate.
    /// - `slot` will not move until it is dropped, i.e. it will be pinned.
    unsafe fn __pinned_init(self, slot: *mut T) -> Result<(), E>;
}

/// An initializer for `T`.
///
/// To use this initializer, you will need a suitable memory location that can hold a `T`. This can
/// be [`Box<T>`], [`Arc<T>`], [`UniqueArc<T>`] or even the stack (see [`stack_pin_init!`]). Use the
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
/// - returns `Ok(())` if it initialized every field of `slot`,
/// - returns `Err(err)` if it encountered an error and then cleaned `slot`, this means:
///     - `slot` can be deallocated without UB occurring,
///     - `slot` does not need to be dropped,
///     - `slot` is not partially initialized.
/// - while constructing the `T` at `slot` it upholds the pinning invariants of `T`.
///
/// The `__pinned_init` function from the supertrait [`PinInit`] needs to execute the exact same
/// code as `__init`.
///
/// Contrary to its supertype [`PinInit<T, E>`] the caller is allowed to
/// move the pointee after initialization.
///
/// [`Arc<T>`]: crate::sync::Arc
#[must_use = "An initializer must be used in order to create its value."]
pub unsafe trait Init<T: ?Sized, E = Infallible>: PinInit<T, E> {
    /// Initializes `slot`.
    ///
    /// # Safety
    ///
    /// - `slot` is a valid pointer to uninitialized memory.
    /// - the caller does not touch `slot` when `Err` is returned, they are only permitted to
    ///   deallocate.
    unsafe fn __init(self, slot: *mut T) -> Result<(), E>;
}

type Invariant<T> = PhantomData<fn(*mut T) -> *mut T>;
// This is the module-internal type implementing `PinInit` and `Init`. It is unsafe to create this
// type, since the closure needs to fulfill the same safety requirement as the
// `__pinned_init`/`__init` functions.
struct InitClosure<F, T: ?Sized, E>(F, Invariant<(E, T)>);

// SAFETY: While constructing the `InitClosure`, the user promised that it upholds the
// `__pinned_init` invariants.
unsafe impl<T: ?Sized, F, E> PinInit<T, E> for InitClosure<F, T, E>
where
    F: FnOnce(*mut T) -> Result<(), E>,
{
    #[inline]
    unsafe fn __pinned_init(self, slot: *mut T) -> Result<(), E> {
        (self.0)(slot)
    }
}

// SAFETY: While constructing the `InitClosure`, the user promised that it upholds the
// `__init` invariants.
unsafe impl<T: ?Sized, F, E> Init<T, E> for InitClosure<F, T, E>
where
    F: FnOnce(*mut T) -> Result<(), E>,
{
    #[inline]
    unsafe fn __init(self, slot: *mut T) -> Result<(), E> {
        (self.0)(slot)
    }
}

/// Creates a new [`PinInit<T, E>`] from the given closure.
///
/// # Safety
///
/// The closure:
/// - returns `Ok(())` if it initialized every field of `slot`,
/// - returns `Err(err)` if it encountered an error and then cleaned `slot`, this means:
///     - `slot` can be deallocated without UB occurring,
///     - `slot` does not need to be dropped,
///     - `slot` is not partially initialized.
/// - may assume that the `slot` does not move if `T: !Unpin`,
/// - while constructing the `T` at `slot` it upholds the pinning invariants of `T`.
#[inline]
pub const unsafe fn pin_init_from_closure<T: ?Sized, E>(
    f: impl FnOnce(*mut T) -> Result<(), E>,
) -> impl PinInit<T, E> {
    InitClosure(f, PhantomData)
}

/// Creates a new [`Init<T, E>`] from the given closure.
///
/// # Safety
///
/// The closure:
/// - returns `Ok(())` if it initialized every field of `slot`,
/// - returns `Err(err)` if it encountered an error and then cleaned `slot`, this means:
///     - `slot` can be deallocated without UB occurring,
///     - `slot` does not need to be dropped,
///     - `slot` is not partially initialized.
/// - the `slot` may move after initialization.
/// - while constructing the `T` at `slot` it upholds the pinning invariants of `T`.
#[inline]
pub const unsafe fn init_from_closure<T: ?Sized, E>(
    f: impl FnOnce(*mut T) -> Result<(), E>,
) -> impl Init<T, E> {
    InitClosure(f, PhantomData)
}

/// Trait facilitating pinned destruction.
///
/// Use [`pinned_drop`] to implement this trait safely:
///
/// ```rust
/// # use kernel::sync::Mutex;
/// use kernel::macros::pinned_drop;
/// use core::pin::Pin;
/// #[pin_data(PinnedDrop)]
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
pub unsafe trait PinnedDrop: __PinData {
    /// Executes the pinned destructor of this type.
    ///
    /// # Safety
    ///
    /// Only call this from `<Self as Drop>::drop`.
    unsafe fn drop(self: Pin<&mut Self>);

    // Used by the `pinned_drop` proc-macro to ensure that only safe operations are used in `drop`.
    // the function should not be called.
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
        // SAFETY: When init errors/panics, slot will get deallocated but not dropped,
        // slot is valid and will not be moved because of the `Pin::new_unchecked`
        unsafe { init.__pinned_init(slot)? };
        // SAFETY: All fields have been initialized.
        Ok(unsafe { Pin::new_unchecked(this.assume_init()) })
    }

    #[inline]
    fn init<E>(init: impl Init<T, E>) -> error::Result<Self>
    where
        Error: From<E>,
    {
        let mut this = Box::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: When init errors/panics, slot will get deallocated but not dropped,
        // slot is valid
        unsafe { init.__init(slot)? };
        // SAFETY: All fields have been initialized.
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
        // SAFETY: When init errors/panics, slot will get deallocated but not dropped,
        // slot is valid and will not be moved because of the `Pin::new_unchecked`.
        unsafe { init.__pinned_init(slot)? };
        // SAFETY: All fields have been initialized.
        Ok(unsafe { Pin::new_unchecked(this.assume_init()) })
    }

    #[inline]
    fn init<E>(init: impl Init<T, E>) -> error::Result<Self>
    where
        Error: From<E>,
    {
        let mut this = UniqueArc::try_new_uninit()?;
        let slot = this.as_mut_ptr();
        // SAFETY: When init errors/panics, slot will get deallocated but not dropped,
        // slot is valid.
        unsafe { init.__init(slot)? };
        // SAFETY: All fields have been initialized.
        Ok(unsafe { this.assume_init() })
    }
}

/// Marker trait for types that can be initialized by writing just zeroes.
///
/// # Safety
///
/// The bit pattern consisting of only zeroes is a valid bit pattern for this type. In other words,
/// this is not UB:
///
/// ```rust,ignore
/// let val: Self = unsafe { core::mem::zeroed() };
/// ```
pub unsafe trait Zeroable {}

/// Create a new zeroed T.
///
/// The returned initializer will write `0x00` to every byte of the given `slot`.
#[inline]
pub fn zeroed<T: Zeroable + Unpin>() -> impl Init<T> {
    // SAFETY: Because `T: Zeroable`, all bytes zero is a valid bit pattern for `T`
    // and because we write all zeroes, the memory is initialized.
    unsafe {
        init_from_closure(|slot: *mut T| {
            slot.write_bytes(0, 1);
            Ok(())
        })
    }
}

/// An initializer that leaves the memory uninitialized.
///
/// The initializer is a no-op. The `slot` memory is not changed.
#[inline]
pub fn uninit<T>() -> impl Init<MaybeUninit<T>> {
    // SAFETY: The memory is allowed to be uninitialized.
    unsafe { init_from_closure(|_| Ok(())) }
}

/// Convert a value into an initializer.
///
/// Directly moves the value into the given `slot`.
///
/// Note that you can just write `field: value,` in all initializer macros. This function's purpose
/// is to provide compatibility with APIs that only have `PinInit`/`Init` as parameters.
#[inline]
pub fn from_value<T>(value: T) -> impl Init<T> {
    // SAFETY: We use the value to initialize the slot.
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
// SAFETY: All primitives that are allowed to be zero.
impl_zeroable!(
    bool, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);
// SAFETY: There is nothing to zero.
impl_zeroable!(core::marker::PhantomPinned, Infallible, ());

// SAFETY: We are allowed to zero padding bytes.
unsafe impl<const N: usize, T: Zeroable> Zeroable for [T; N] {}

// SAFETY: There is nothing to zero.
unsafe impl<T: ?Sized> Zeroable for PhantomData<T> {}

// SAFETY: `null` pointer is valid.
unsafe impl<T: ?Sized> Zeroable for *mut T {}
unsafe impl<T: ?Sized> Zeroable for *const T {}

macro_rules! impl_tuple_zeroable {
    ($(,)?) => {};
    ($first:ident, $($t:ident),* $(,)?) => {
        // SAFETY: All elements are zeroable and padding can be zero.
        unsafe impl<$first: Zeroable, $($t: Zeroable),*> Zeroable for ($first, $($t),*) {}
        impl_tuple_zeroable!($($t),* ,);
    }
}

impl_tuple_zeroable!(A, B, C, D, E, F, G, H, I, J);

// This trait is only implemented via the `#[pin_data]` proc-macro. It is used to facilitate
// the pin projections within the initializers.
#[doc(hidden)]
pub unsafe trait __PinData {
    type __PinData;
}

/// Stack initializer helper type. Use [`stack_pin_init`] instead of this primitive.
///
/// # Invariants
///
/// If `self.1` is true, then `self.0` is initialized.
///
/// [`stack_pin_init`]: kernel::stack_pin_init
pub struct StackInit<T>(MaybeUninit<T>, bool);

impl<T> Drop for StackInit<T> {
    #[inline]
    fn drop(&mut self) {
        if self.1 {
            // SAFETY: As we are being dropped, we only call this once. And since `self.1 == true`,
            // `self.0` has to be initialized.
            unsafe { self.0.assume_init_drop() };
        }
    }
}
impl<T> StackInit<T> {
    /// Creates a new [`StackInit<T>`] that is uninitialized. Use [`stack_pin_init`] instead of this
    /// primitive.
    ///
    /// [`stack_pin_init`]: kernel::stack_pin_init
    #[inline]
    pub fn uninit() -> Self {
        Self(MaybeUninit::uninit(), false)
    }

    /// Initializes the contents and returns the result.
    ///
    /// # Safety
    ///
    /// The caller ensures that `self` is on the stack and not accessible in any other way, if this
    /// function returns `Ok`.
    #[inline]
    pub unsafe fn init<E>(&mut self, init: impl PinInit<T, E>) -> Result<Pin<&mut T>, E> {
        // SAFETY: The memory slot is valid and this type ensures that it will stay pinned.
        unsafe { init.__pinned_init(self.0.as_mut_ptr())? };
        self.1 = true;
        // SAFETY: The slot is now pinned, since we will never give access to `&mut T`.
        Ok(unsafe { Pin::new_unchecked(self.0.assume_init_mut()) })
    }
}

/// When a value of this type is dropped, it drops a `T`.
///
/// Public, but hidden type, since it should only be used by the macros of this module.
#[doc(hidden)]
pub struct DropGuard<T: ?Sized>(*mut T, Cell<bool>);

impl<T: ?Sized> DropGuard<T> {
    /// Creates a new [`DropGuard<T>`]. It will [`ptr::drop_in_place`] `ptr` when it gets dropped.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid pointer.
    ///
    /// It is the callers responsibility that `self` will only get dropped if the pointee of `ptr`:
    /// - has not been dropped,
    /// - is not accessible by any other means,
    /// - will not be dropped by any other means.
    #[inline]
    pub unsafe fn new(ptr: *mut T) -> Self {
        Self(ptr, Cell::new(true))
    }

    /// Prevents this guard from dropping the supplied pointer.
    ///
    /// # Safety
    ///
    /// This function is unsafe in order to prevent safe code from forgetting this guard. It should
    /// only be called by the macros in this module.
    #[inline]
    pub unsafe fn forget(&self) {
        self.1.set(false);
    }
}

impl<T: ?Sized> Drop for DropGuard<T> {
    #[inline]
    fn drop(&mut self) {
        if self.1.get() {
            // SAFETY: A `DropGuard` can only be constructed using the unsafe `new` function
            // ensuring that this operation is safe.
            unsafe { ptr::drop_in_place(self.0) }
        }
    }
}
