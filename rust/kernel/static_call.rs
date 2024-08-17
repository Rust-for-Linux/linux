// SPDX-License-Identifier: GPL-2.0

// Copyright (C) 2024 Google LLC.

//! Logic for static calls.

#[macro_export]
#[doc(hidden)]
macro_rules! ty_underscore_for {
    ($arg:expr) => {
        _
    };
}

#[doc(hidden)]
#[repr(transparent)]
pub struct AddressableStaticCallKey {
    _ptr: *const bindings::static_call_key,
}
unsafe impl Sync for AddressableStaticCallKey {}
impl AddressableStaticCallKey {
    pub const fn new(ptr: *const bindings::static_call_key) -> Self {
        Self { _ptr: ptr }
    }
}

#[cfg(CONFIG_HAVE_STATIC_CALL)]
#[doc(hidden)]
#[macro_export]
macro_rules! _static_call {
    ($name:ident($($args:expr),* $(,)?)) => {{
        // Symbol mangling will give this symbol a unique name.
        #[cfg(CONFIG_HAVE_STATIC_CALL_INLINE)]
        #[link_section = ".discard.addressable"]
        #[used]
        static __ADDRESSABLE: $crate::static_call::AddressableStaticCallKey = unsafe {
            $crate::static_call::AddressableStaticCallKey::new(::core::ptr::addr_of!(
                $crate::macros::paste! { $crate::bindings:: [<__SCK__ $name >]; }
            ))
        };

        let fn_ptr: unsafe extern "C" fn($($crate::static_call::ty_underscore_for!($args)),*) -> _ =
            $crate::macros::paste! { $crate::bindings:: [<__SCT__ $name >]; };
        (fn_ptr)($($args),*)
    }};
}

#[cfg(not(CONFIG_HAVE_STATIC_CALL))]
#[doc(hidden)]
#[macro_export]
macro_rules! _static_call {
    ($name:ident($($args:expr),* $(,)?)) => {{
        let void_ptr_fn: *mut ::core::ffi::c_void = $crate::macros::paste! { $crate::bindings:: [<__SCK__ $name >]; }.func;

        let fn_ptr: unsafe extern "C" fn($($crate::static_call::ty_underscore_for!($args)),*) -> _ = if true {
            ::core::mem::transmute(void_ptr_fn)
        } else {
            // This is dead code, but it influences type inference on `fn_ptr` so that we transmute
            // the function pointer to the right type.
            $crate::macros::paste! { $crate::bindings:: [<__SCT__ $name >]; }
        };

        (fn_ptr)($($args),*)
    }};
}

/// Statically call a global function.
///
/// # Safety
///
/// This macro will call the provided function. It is up to the caller to uphold the safety
/// guarantees of the function.
///
/// # Examples
///
/// ```ignore
/// fn call_static() {
///     unsafe {
///         static_call! { your_static_call() };
///     }
/// }
/// ```
#[macro_export]
macro_rules! static_call {
    // Forward to the real implementation. Separated like this so that we don't have to duplicate
    // the documentation.
    ($($args:tt)*) => { $crate::static_call::_static_call! { $($args)* } };
}

pub use {_static_call, static_call, ty_underscore_for};
