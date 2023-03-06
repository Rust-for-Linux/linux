// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module provides the macros that actually implement the proc-macros `pin_data` and
//! `pinned_drop`. These macros should never be called directly, since they expect their input to be
//! in a certain format which is internal. Use the proc-macros instead.
//!
//! This architecture has been chosen because the kernel does not yet have access to `syn` which
//! would make matters a lot easier for implementing these as proc-macros.

/// This macro creates a `unsafe impl<...> PinnedDrop for $type` block.
///
/// See [`PinnedDrop`] for more information.
#[doc(hidden)]
#[macro_export]
macro_rules! __pinned_drop {
    (
        @impl_sig($($impl_sig:tt)*),
        @impl_body(
            $(#[$($attr:tt)*])*
            fn drop($self:ident: $st:ty) {
                $($inner:stmt)*
            }
        ),
    ) => {
        unsafe $($impl_sig)* {
            // Inherit all attributes and the type/ident tokens for the signature.
            $(#[$($attr)*])*
            unsafe fn drop($self: $st) {
                $($inner)*
            }

            // The `drop` function has to be `unsafe`, since calling it twice is UB. But the
            // `drop` function the user declared is safe. We need to prevent users from doing
            // `unsafe` operations inside the body. Hence we declare this function which should not
            // be called (see the `PinnedDrop` trait) and which does nothing, we rely on type
            // checking to catch `unsafe` operations without `unsafe` blocks.
            fn __ensure_no_unsafe_op_in_drop($self: $st) {
                if false {
                    $($inner)*
                }
            }
        }
    }
}

/// This macro first parses the struct definition such that it separates pinned and not pinned
/// fields. Afterwards it declares the struct and implement the `__PinData` trait safely.
#[doc(hidden)]
#[macro_export]
macro_rules! __pin_data {
    // Proc-macro entry point, this is supplied by the proc-macro pre-parsing.
    (parse_input:
        @args($($pinned_drop:ident)?),
        @sig(
            $(#[$($struct_attr:tt)*])*
            $vis:vis struct $name:ident
            $(where $($whr:tt)*)?
        ),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @body({ $($fields:tt)* }),
    ) => {
        // We now use token munching to iterate through all of the fields. While doing this we
        // identify fields marked with `#[pin]`, these fields are the 'pinned fields'. The user
        // wants these to be structurally pinned. The rest of the fields are the
        // 'not pinned fields'. Additionally we collect all fields, since we need them in the right
        // order to declare the struct.
        //
        // In this call we also put some explaining comments for the parameters.
        $crate::__pin_data!(find_pinned_fields:
            // Attributes on the struct itself, these will just be propagated to be put onto the
            // struct definition.
            @struct_attrs($(#[$($struct_attr)*])*),
            // The visibility of the struct.
            @vis($vis),
            // The name of the struct.
            @name($name),
            // The 'impl generics', the generics that will need to be specified on the struct inside
            // of an `impl<$ty_generics>` block.
            @impl_generics($($impl_generics)*),
            // The 'ty generics', the generics that will need to be specified on the impl blocks.
            @ty_generics($($ty_generics)*),
            // The where clause of any impl block and the declaration.
            @where($($($whr)*)?),
            // The remaining fields tokens that need to be processed.
            // We add a `,` at the end to ensure correct parsing.
            @fields_munch($($fields)* ,),
            // The pinned fields.
            @pinned(),
            // The not pinned fields.
            @not_pinned(),
            // All fields.
            @fields(),
            // The accumulator containing all attributes already parsed.
            @accum(),
            // Contains `yes` or `` to indicate if `#[pin]` was found on the current field.
            @is_pinned(),
            // The proc-macro argument, this should be `PinnedDrop` or ``.
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // We reached the field declaration.
        @fields_munch($field:ident : $type:ty, $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        // This field is pinned.
        @is_pinned(yes),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::__pin_data!(find_pinned_fields:
            @struct_attrs($($struct_attrs)*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($whr)*),
            @fields_munch($($rest)*),
            @pinned($($pinned)* $($accum)* $field: $type,),
            @not_pinned($($not_pinned)*),
            @fields($($fields)* $($accum)* $field: $type,),
            @accum(),
            @is_pinned(),
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // We reached the field declaration.
        @fields_munch($field:ident : $type:ty, $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        // This field is not pinned.
        @is_pinned(),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::__pin_data!(find_pinned_fields:
            @struct_attrs($($struct_attrs)*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($whr)*),
            @fields_munch($($rest)*),
            @pinned($($pinned)*),
            @not_pinned($($not_pinned)* $($accum)* $field: $type,),
            @fields($($fields)* $($accum)* $field: $type,),
            @accum(),
            @is_pinned(),
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // We found the `#[pin]` attr.
        @fields_munch(#[pin] $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::__pin_data!(find_pinned_fields:
            @struct_attrs($($struct_attrs)*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($whr)*),
            @fields_munch($($rest)*),
            // We do not include `#[pin]` in the list of attributes, since it is not actually an
            // attribute that is defined somewhere.
            @pinned($($pinned)*),
            @not_pinned($($not_pinned)*),
            @fields($($fields)*),
            @accum($($accum)*),
            // Set this to `yes`.
            @is_pinned(yes),
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // We reached the field declaration with visibility, for simplicity we only munch the
        // visibility and put it into `$accum`.
        @fields_munch($fvis:vis $field:ident $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::__pin_data!(find_pinned_fields:
            @struct_attrs($($struct_attrs)*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($whr)*),
            @fields_munch($field $($rest)*),
            @pinned($($pinned)*),
            @not_pinned($($not_pinned)*),
            @fields($($fields)*),
            @accum($($accum)* $fvis),
            @is_pinned($($is_pinned)?),
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // Some other attribute, just put it into `$accum`.
        @fields_munch(#[$($attr:tt)*] $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::__pin_data!(find_pinned_fields:
            @struct_attrs($($struct_attrs)*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($whr)*),
            @fields_munch($($rest)*),
            @pinned($($pinned)*),
            @not_pinned($($not_pinned)*),
            @fields($($fields)*),
            @accum($($accum)* #[$($attr)*]),
            @is_pinned($($is_pinned)?),
            @pinned_drop($($pinned_drop)?),
        );
    };
    (find_pinned_fields:
        @struct_attrs($($struct_attrs:tt)*),
        @vis($vis:vis),
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        // We reached the end of the fields, plus an optional additional comma, since we added one
        // before and the user is also allowed to put a trailing comma.
        @fields_munch($(,)?),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum(),
        @is_pinned(),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        // Declare the struct with all fields in the correct order.
        $($struct_attrs)*
        $vis struct $name <$($impl_generics)*>
        where $($whr)*
        {
            $($fields)*
        }

        // We put the rest into this const item, because it then will not be accessible to anything
        // outside.
        const _: () = {
            // We declare this struct which will host all of the projection function for our type.
            // it will be invariant over all generic parameters which are inherited from the
            // struct.
            $vis struct __ThePinData<$($impl_generics)*>
            where $($whr)*
            {
                __phantom: ::core::marker::PhantomData<
                    fn($name<$($ty_generics)*>) -> $name<$($ty_generics)*>
                >,
            }

            // Make all projection functions.
            $crate::__pin_data!(make_pin_data:
                @pin_data(__ThePinData),
                @impl_generics($($impl_generics)*),
                @ty_generics($($ty_generics)*),
                @where($($whr)*),
                @pinned($($pinned)*),
                @not_pinned($($not_pinned)*),
            );

            // SAFETY: We have added the correct projection functions above to `__ThePinData` and
            // we also use the least restrictive generics possible.
            unsafe impl<$($impl_generics)*> $crate::init::__PinData for $name<$($ty_generics)*>
            where $($whr)*
            {
                type __PinData = __ThePinData<$($ty_generics)*>;
            }

            // This struct will be used for the unpin analysis. Since only structurally pinned
            // fields are relevant whether the struct should implement `Unpin`.
            #[allow(dead_code)]
            struct __Unpin <'__pin, $($impl_generics)*>
            where $($whr)*
            {
                __phantom_pin: ::core::marker::PhantomData<fn(&'__pin ()) -> &'__pin ()>,
                __phantom: ::core::marker::PhantomData<
                    fn($name<$($ty_generics)*>) -> $name<$($ty_generics)*>
                >,
                // Only the pinned fields.
                $($pinned)*
            }

            #[doc(hidden)]
            impl<'__pin, $($impl_generics)*> ::core::marker::Unpin for $name<$($ty_generics)*>
            where
                __Unpin<'__pin, $($ty_generics)*>: ::core::marker::Unpin,
                $($whr)*
            {}

            // We need to disallow normal `Drop` implementation, the exact behavior depends on
            // whether `PinnedDrop` was specified as the parameter.
            $crate::__pin_data!(drop_prevention:
                @name($name),
                @impl_generics($($impl_generics)*),
                @ty_generics($($ty_generics)*),
                @where($($whr)*),
                @pinned_drop($($pinned_drop)?),
            );
        };
    };
    // When no `PinnedDrop` was specified, then we have to prevent implementing drop.
    (drop_prevention:
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned_drop(),
    ) => {
        // We prevent this by creating a trait that will be implemented for all types implementing
        // `Drop`. Additionally we will implement this trait for the struct leading to a conflict,
        // if it also implements `Drop`
        trait MustNotImplDrop {}
        #[allow(drop_bounds)]
        impl<T: ::core::ops::Drop> MustNotImplDrop for T {}
        impl<$($impl_generics)*> MustNotImplDrop for $name<$($ty_generics)*>
        where $($whr)* {}
        // We also take care to prevent users from writing a useless PinnedDrop implementation.
        // They might implement PinnedDrop correctly for the struct, but forget to give
        // `PinnedDrop` as the parameter to `#[pin_data]`.
        #[allow(non_camel_case_types)]
        trait UselessPinnedDropImpl_you_need_to_specify_PinnedDrop {}
        impl<T: $crate::init::PinnedDrop>
            UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for T {}
        impl<$($impl_generics)*>
            UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for $name<$($ty_generics)*>
        where $($whr)* {}
    };
    // When `PinnedDrop` was specified we just implement drop and delegate.
    (drop_prevention:
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned_drop(PinnedDrop),
    ) => {
        impl<$($impl_generics)*> ::core::ops::Drop for $name<$($ty_generics)*>
        where $($whr)*
        {
            fn drop(&mut self) {
                let pinned = unsafe { ::core::pin::Pin::new_unchecked(self) };
                unsafe { $crate::init::PinnedDrop::drop(pinned) }
            }
        }
    };
    // If some other parameter was specified, we emit a readable error.
    (drop_prevention:
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned_drop($($rest:tt)*),
    ) => {
        compile_error!(
            "Wrong parameters to `#[pin_data]`, expected nothing or `PinnedDrop`, got '{}'.",
            stringify!($($rest)*),
        );
    };
    (make_pin_data:
        @pin_data($pin_data:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned($($(#[$($p_attr:tt)*])* $pvis:vis $p_field:ident : $p_type:ty),* $(,)?),
        @not_pinned($($(#[$($attr:tt)*])* $fvis:vis $field:ident : $type:ty),* $(,)?),
    ) => {
        // For every field, we create a projection function according to its projection type. If a
        // field is structurally pinned, then it must be initialized via `PinInit`, if it is not
        // structurally pinned, then it can be initialized via `Init`.
        //
        // The functions are `unsafe` to prevent accidentally calling them.
        #[allow(dead_code)]
        impl<$($impl_generics)*> $pin_data<$($ty_generics)*>
        where $($whr)*
        {
            $(
                $pvis unsafe fn $p_field<E>(
                    slot: *mut $p_type,
                    init: impl $crate::init::PinInit<$p_type, E>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { $crate::init::PinInit::__pinned_init(init, slot) }
                }
            )*
            $(
                $fvis unsafe fn $field<E>(
                    slot: *mut $type,
                    init: impl $crate::init::Init<$type, E>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { $crate::init::Init::__init(init, slot) }
                }
            )*
        }
    };
}
