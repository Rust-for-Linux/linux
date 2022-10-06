// SPDX-License-Identifier: GPL-2.0

// use the proc macro instead
#[doc(hidden)]
#[macro_export]
macro_rules! pinned_drop {
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
            $(#[$($attr)*])*
            unsafe fn drop($self: $st) {
                $($inner)*
            }

            fn __ensure_no_unsafe_op_in_drop($self: $st) {
                if false {
                    $($inner)*
                }
            }
        }
    }
}
