// SPDX-License-Identifier: GPL-2.0

// use the proc macro instead
#[doc(hidden)]
#[macro_export]
macro_rules! pin_project {
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
        $crate::pin_project!(find_pinned_fields:
            @struct_attrs($(#[$($struct_attr)*])*),
            @vis($vis),
            @name($name),
            @impl_generics($($impl_generics)*),
            @ty_generics($($ty_generics)*),
            @where($($($whr)*)?),
            @fields_munch($($fields)*),
            @pinned(),
            @not_pinned(),
            @fields(),
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
        @fields_munch($field:ident : $type:ty, $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned(yes),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::pin_project!(find_pinned_fields:
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
        @fields_munch($field:ident : $type:ty, $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned(),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::pin_project!(find_pinned_fields:
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
        @fields_munch(#[pin] $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::pin_project!(find_pinned_fields:
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
            @accum($($accum)*),
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
        @fields_munch($fvis:vis $field:ident $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::pin_project!(find_pinned_fields:
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
        @fields_munch(#[$($attr:tt)*] $($rest:tt)*),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum($($accum:tt)*),
        @is_pinned($($is_pinned:ident)?),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $crate::pin_project!(find_pinned_fields:
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
        @fields_munch(),
        @pinned($($pinned:tt)*),
        @not_pinned($($not_pinned:tt)*),
        @fields($($fields:tt)*),
        @accum(),
        @is_pinned(),
        @pinned_drop($($pinned_drop:ident)?),
    ) => {
        $($struct_attrs)*
        $vis struct $name <$($impl_generics)*>
        where $($whr)*
        {
            $($fields)*
        }

        const _: () = {
            $vis struct __ThePinData<$($impl_generics)*>
            where $($whr)*
            {
                __phantom: ::core::marker::PhantomData<fn($name<$($ty_generics)*>) -> $name<$($ty_generics)*>>,
            }

            $crate::pin_project!(make_pin_data:
                @pin_data(__ThePinData),
                @impl_generics($($impl_generics)*),
                @ty_generics($($ty_generics)*),
                @where($($whr)*),
                @pinned($($pinned)*),
                @not_pinned($($not_pinned)*),
            );

            unsafe impl<$($impl_generics)*> $crate::init::__private::__PinData for $name<$($ty_generics)*>
            where $($whr)*
            {
                type __PinData = __ThePinData<$($ty_generics)*>;
            }

            #[allow(dead_code)]
            struct __Unpin <'__pin, $($impl_generics)*>
            where $($whr)*
            {
                __phantom_pin: ::core::marker::PhantomData<fn(&'__pin ()) -> &'__pin ()>,
                __phantom: ::core::marker::PhantomData<fn($name<$($ty_generics)*>) -> $name<$($ty_generics)*>>,
                $($pinned)*
            }

            #[doc(hidden)]
            impl<'__pin, $($impl_generics)*> ::core::marker::Unpin for $name<$($ty_generics)*>
            where
                __Unpin<'__pin, $($ty_generics)*>: ::core::marker::Unpin,
                $($whr)*
            {}

            $crate::pin_project!(drop_prevention:
                @name($name),
                @impl_generics($($impl_generics)*),
                @ty_generics($($ty_generics)*),
                @where($($whr)*),
                @pinned_drop($($pinned_drop)?),
            );
        };
    };
    (drop_prevention:
        @name($name:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned_drop(),
    ) => {
        trait MustNotImplDrop {}
        #[allow(drop_bounds)]
        impl<T: ::core::ops::Drop> MustNotImplDrop for T {}
        impl<$($impl_generics)*> MustNotImplDrop for $name<$($ty_generics)*>
        where $($whr)*
        {}
        #[allow(non_camel_case_types)]
        trait UselessPinnedDropImpl_you_need_to_specify_PinnedDrop {}
        impl<T: $crate::init::PinnedDrop> UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for T {}
        impl<$($impl_generics)*> UselessPinnedDropImpl_you_need_to_specify_PinnedDrop for $name<$($ty_generics)*>
        where
            $($whr)*
        {}
    };
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
    (make_pin_data:
        @pin_data($pin_data:ident),
        @impl_generics($($impl_generics:tt)*),
        @ty_generics($($ty_generics:tt)*),
        @where($($whr:tt)*),
        @pinned($($(#[$($p_attr:tt)*])* $pvis:vis $p_field:ident : $p_type:ty),* $(,)?),
        @not_pinned($($(#[$($attr:tt)*])* $fvis:vis $field:ident : $type:ty),* $(,)?),
    ) => {
        #[allow(dead_code)]
        impl<$($impl_generics)*> $pin_data<$($ty_generics)*>
        where $($whr)*
        {
            $(
                $pvis unsafe fn $p_field<E, W: $crate::init::__private::InitWay>(
                    slot: *mut $p_type,
                    init: impl $crate::init::__private::__PinInitImpl<$p_type, E, W>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { $crate::init::__private::__PinInitImpl::__pinned_init(init, slot) }
                }
            )*
            $(
                $fvis unsafe fn $field<E, W: $crate::init::__private::InitWay>(
                    slot: *mut $type,
                    init: impl $crate::init::__private::__InitImpl<$type, E, W>,
                ) -> ::core::result::Result<(), E> {
                    unsafe { $crate::init::__private::__InitImpl::__init(init, slot) }
                }
            )*
        }
    };
}
