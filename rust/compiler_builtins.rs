// SPDX-License-Identifier: GPL-2.0

//! Our own `compiler_builtins`.
//!
//! Rust provides [`compiler_builtins`] as a port of LLVM's [`compiler-rt`].
//! Since we do not need the vast majority of them, we avoid the dependency
//! by providing this file.
//!
//! At the moment, some builtins are required that should not be. For instance,
//! [`core`] has 128-bit integers functionality which we should not be compiling
//! in. We will work with upstream [`core`] to provide feature flags to disable
//! the parts we do not need. For the moment, we define them to [`panic!`] at
//! runtime for simplicity to catch mistakes, instead of performing surgery
//! on `core.o`.
//!
//! In any case, all these symbols are weakened to ensure we do not override
//! those that may be provided by the rest of the kernel.
//!
//! [`compiler_builtins`]: https://github.com/rust-lang/compiler-builtins
//! [`compiler-rt`]: https://compiler-rt.llvm.org/

#![feature(compiler_builtins)]
#![feature(naked_functions)]
#![compiler_builtins]
#![no_builtins]
#![no_std]

macro_rules! define_panicking_intrinsics(
    ($reason: tt, { $($ident: ident, )* }) => {
        $(
            #[doc(hidden)]
            #[export_name = concat!("__rust", stringify!($ident))]
            pub extern "C" fn $ident() {
                panic!($reason);
            }
        )*
    }
);

define_panicking_intrinsics!("`f32` should not be used", {
    __eqsf2,
    __gesf2,
    __lesf2,
    __nesf2,
    __unordsf2,
});

define_panicking_intrinsics!("`f64` should not be used", {
    __unorddf2,
});

define_panicking_intrinsics!("`i128` should not be used", {
    __ashrti3,
    __muloti4,
    __multi3,
});

define_panicking_intrinsics!("`u128` should not be used", {
    __ashlti3,
    __lshrti3,
    __udivmodti4,
    __udivti3,
    __umodti3,
});

#[cfg(target_arch = "arm")]
define_panicking_intrinsics!("`f32` should not be used", {
    __aeabi_fcmpeq,
    __aeabi_fcmpun,
});

#[cfg(target_arch = "arm")]
define_panicking_intrinsics!("`f64` should not be used", {
    __aeabi_dcmpun,
});

#[cfg(target_arch = "arm")]
#[doc(hidden)]
#[naked]
#[no_mangle]
pub unsafe extern "C" fn __rust__aeabi_uldivmod() {
    // Adapted from compiler-rt:
    // https://github.com/llvm/llvm-project/blob/a80e65e00ada7a9c16acf17a5fd40b4f12ced3a8/compiler-rt/lib/builtins/arm/aeabi_uldivmod.S
    unsafe {
        core::arch::asm!(
            "push {{r6, lr}}",
            "sub  sp, sp, #16",
            "add  r6, sp, #8",
            "str  r6, [sp]",
            "bl   div64_u64_rem",
            "ldr  r2, [sp, #8]",
            "ldr  r3, [sp, #12]",
            "add  sp, sp, #16",
            "pop  {{r6, pc}}",
            options(noreturn)
        );
    }
}
