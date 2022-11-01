// SPDX-License-Identifier: GPL-2.0

//! Module containing common kernel initializer functions.

use crate::{
    init::{self, PinInit},
    Opaque,
};

macro_rules! create_func {
    ($name:ident $(, $arg_name:ident: $arg_typ:ident)*) => {
        /// Create an initializer using the given initalizer function from C.
        ///
        /// # Safety
        /// The given function **must** under all circumstances initialize the memory location to a valid
        /// `T`. If it fails to do so it results in UB.
        ///
        /// If any parameters are given, those need to be valid for the function. Valid means that
        /// calling the function with those parameters complies with the above requirement **and** every
        /// other requirement on the function itself.
        pub unsafe fn $name<T $(, $arg_typ)*>(init_func: unsafe extern "C" fn(*mut T $(, $arg_name: $arg_typ)*) $(, $arg_name: $arg_typ)*) -> impl PinInit<Opaque<T>> {
            unsafe {
                init::pin_init_from_closure(move |slot| {
                    init_func(Opaque::raw_get(slot) $(, $arg_name)*);
                    Ok(())
                })
            }
        }
    }
}

create_func!(ffi_init);
create_func!(ffi_init1, arg1: A1);
create_func!(ffi_init2, arg1: A1, arg2: A2);
create_func!(ffi_init3, arg1: A1, arg2: A2, arg3: A3);
create_func!(ffi_init4, arg1: A1, arg2: A2, arg3: A3, arg4: A4);
