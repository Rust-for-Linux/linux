// SPDX-License-Identifier: GPL-2.0

// Copyright (C) 2024 Google LLC.

//! Logic for static keys.

use crate::bindings::*;

#[doc(hidden)]
#[macro_export]
#[cfg(target_arch = "x86_64")]
#[cfg(not(CONFIG_HAVE_RUST_ASM_GOTO))]
macro_rules! _static_key_false {
    ($key:path, $keytyp:ty, $field:ident) => {{
        let mut output = 1u32;

        core::arch::asm!(
            r#"
            1: .byte 0x0f,0x1f,0x44,0x00,0x00

            .pushsection __jump_table,  "aw"
            .balign 8
            .long 1b - .
            .long 3f - .
            .quad {0} + {1} - .
            .popsection

            2: mov {2:e}, 0
            3:
            "#,
            sym $key,
            const ::core::mem::offset_of!($keytyp, $field),
            inout(reg) output,
        );

        output != 0
    }};
}

#[doc(hidden)]
#[macro_export]
#[cfg(target_arch = "x86_64")]
#[cfg(CONFIG_HAVE_RUST_ASM_GOTO)]
macro_rules! _static_key_false {
    ($key:path, $keytyp:ty, $field:ident) => {'my_label: {
        core::arch::asm!(
            r#"
            1: .byte 0x0f,0x1f,0x44,0x00,0x00

            .pushsection __jump_table,  "aw"
            .balign 8
            .long 1b - .
            .long {0} - .
            .quad {1} + {2} - .
            .popsection
            "#,
            label {
                break 'my_label true;
            },
            sym $key,
            const ::core::mem::offset_of!($keytyp, $field),
        );

        break 'my_label false;
    }};
}

#[doc(hidden)]
#[macro_export]
#[cfg(target_arch = "aarch64")]
#[cfg(not(CONFIG_HAVE_RUST_ASM_GOTO))]
macro_rules! _static_key_false {
    ($key:path, $keytyp:ty, $field:ident) => {{
        let mut output = 1u32;

        core::arch::asm!(
            r#"
            1: nop

            .pushsection __jump_table,  "aw"
            .align 3
            .long 1b - ., 3f - .
            .quad {0} + {1} - .
            .popsection

            2: mov {2:w}, 0
            3:
            "#,
            sym $key,
            const ::core::mem::offset_of!($keytyp, $field),
            inout(reg) output
        );

        output != 0
    }};
}

#[doc(hidden)]
#[macro_export]
#[cfg(target_arch = "aarch64")]
#[cfg(CONFIG_HAVE_RUST_ASM_GOTO)]
macro_rules! _static_key_false {
    ($key:path, $keytyp:ty, $field:ident) => {'my_label: {
        core::arch::asm!(
            r#"
            1: nop

            .pushsection __jump_table,  "aw"
            .align 3
            .long 1b - ., {0} - .
            .quad {1} + {2} - .
            .popsection
            "#,
            label {
                break 'my_label true;
            },
            sym $key,
            const ::core::mem::offset_of!($keytyp, $field),
        );

        break 'my_label false;
    }};
}

/// Branch based on a static key.
///
/// Takes three arguments:
///
/// * `key` - the path to the static variable containing the `static_key`.
/// * `keytyp` - the type of `key`.
/// * `field` - the name of the field of `key` that contains the `static_key`.
#[macro_export]
macro_rules! static_key_false {
    // Forward to the real implementation. Separated like this so that we don't have to duplicate
    // the documentation.
    ($key:path, $keytyp:ty, $field:ident) => {{
        // Assert that `$key` has type `$keytyp` and that `$key.$field` has type `static_key`.
        //
        // SAFETY: We know that `$key` is a static because otherwise the inline assembly will not
        // compile. The raw pointers created in this block are in-bounds of `$key`.
        static _TY_ASSERT: () = unsafe {
            let key: *const $keytyp = ::core::ptr::addr_of!($key);
            let _: *const $crate::bindings::static_key = ::core::ptr::addr_of!((*key).$field);
        };

        $crate::static_key::_static_key_false! { $key, $keytyp, $field }
    }};
}

pub use {_static_key_false, static_key_false};
