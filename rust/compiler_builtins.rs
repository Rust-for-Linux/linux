// SPDX-License-Identifier: GPL-2.0

//! Our own `compiler_builtins`.
//!
//! Rust provides [`compiler_builtins`] as a port of LLVM's [`compiler-rt`].
//! Since we do not need the vast majority of them, we avoid the dependency
//! by providing this file.
//!
//! At the moment, some builtins are required that should not be. For instance,
//! [`core`] has floating-point functionality which we should not be compiling
//! in. For the moment, we define them to [`panic!`] at runtime for simplicity.
//! These are actually a superset of the ones we actually need to define,
//! but it seems simpler to ban entire categories at once. In the future,
//! we might be able to remove all this by providing our own custom [`core`]
//! etc., or perhaps [`core`] itself might provide `cfg` options to disable
//! enough functionality to avoid requiring some of these.
//!
//! In any case, all these symbols are weakened to ensure we do not override
//! those that may be provided by the rest of the kernel.
//!
//! [`compiler_builtins`]: https://github.com/rust-lang/compiler-builtins
//! [`compiler-rt`]: https://compiler-rt.llvm.org/

#![feature(compiler_builtins)]
#![compiler_builtins]
#![no_builtins]
#![no_std]
#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![deny(clippy::perf)]
#![deny(clippy::style)]

macro_rules! define_panicking_intrinsics(
    ($reason: tt, { $($ident: ident, )* }) => {
        $(
            #[doc(hidden)]
            #[no_mangle]
            pub extern "C" fn $ident() {
                panic!($reason);
            }
        )*
    }
);

define_panicking_intrinsics!("non-inline stack probes should not be used", {
    __rust_probestack,
});

define_panicking_intrinsics!("`f32` should not be used", {
    __addsf3,
    __addsf3vfp,
    __divsf3,
    __divsf3vfp,
    __eqsf2,
    __eqsf2vfp,
    __fixsfdi,
    __fixsfsi,
    __fixsfti,
    __fixunssfdi,
    __fixunssfsi,
    __fixunssfti,
    __floatdisf,
    __floatsisf,
    __floattisf,
    __floatundisf,
    __floatunsisf,
    __floatuntisf,
    __gesf2,
    __gesf2vfp,
    __gtsf2,
    __gtsf2vfp,
    __lesf2,
    __lesf2vfp,
    __ltsf2,
    __ltsf2vfp,
    __mulsf3,
    __mulsf3vfp,
    __nesf2,
    __nesf2vfp,
    __powisf2,
    __subsf3,
    __subsf3vfp,
    __unordsf2,
});

define_panicking_intrinsics!("`f64` should not be used", {
    __adddf3,
    __adddf3vfp,
    __divdf3,
    __divdf3vfp,
    __eqdf2,
    __eqdf2vfp,
    __fixdfdi,
    __fixdfsi,
    __fixdfti,
    __fixunsdfdi,
    __fixunsdfsi,
    __fixunsdfti,
    __floatdidf,
    __floatsidf,
    __floattidf,
    __floatundidf,
    __floatunsidf,
    __floatuntidf,
    __gedf2,
    __gedf2vfp,
    __gtdf2,
    __gtdf2vfp,
    __ledf2,
    __ledf2vfp,
    __ltdf2,
    __ltdf2vfp,
    __muldf3,
    __muldf3vfp,
    __nedf2,
    __nedf2vfp,
    __powidf2,
    __subdf3,
    __subdf3vfp,
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
