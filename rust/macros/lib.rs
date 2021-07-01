// SPDX-License-Identifier: GPL-2.0

//! Crate for all kernel procedural macros.

#![deny(clippy::complexity)]
#![deny(clippy::correctness)]
#![deny(clippy::perf)]
#![deny(clippy::style)]

mod module;

use proc_macro::TokenStream;

/// Declares a kernel module.
///
/// The `type` argument should be a type which implements the [`KernelModule`]
/// trait. Also accepts various forms of kernel metadata.
///
/// C header: [`include/linux/moduleparam.h`](../../../include/linux/moduleparam.h)
///
/// [`KernelModule`]: ../kernel/trait.KernelModule.html
///
/// # Examples
///
/// ```ignore
/// use kernel::prelude::*;
///
/// module!{
///     type: MyKernelModule,
///     name: b"my_kernel_module",
///     author: b"Rust for Linux Contributors",
///     description: b"My very own kernel module!",
///     license: b"GPL v2",
///     params: {
///        my_i32: i32 {
///            default: 42,
///            permissions: 0o000,
///            description: b"Example of i32",
///        },
///        writeable_i32: i32 {
///            default: 42,
///            permissions: 0o644,
///            description: b"Example of i32",
///        },
///    },
/// }
///
/// struct MyKernelModule;
///
/// impl KernelModule for MyKernelModule {
///     fn init() -> Result<Self> {
///         // If the parameter is writeable, then the kparam lock must be
///         // taken to read the parameter:
///         {
///             let lock = THIS_MODULE.kernel_param_lock();
///             pr_info!("i32 param is:  {}\n", writeable_i32.read(&lock));
///         }
///         // If the parameter is read only, it can be read without locking
///         // the kernel parameters:
///         pr_info!("i32 param is:  {}\n", my_i32.read());
///         Ok(MyKernelModule)
///     }
/// }
/// ```
///
/// # Supported argument types
///   - `type`: type which implements the [`KernelModule`] trait (required).
///   - `name`: byte array of the name of the kernel module (required).
///   - `author`: byte array of the author of the kernel module.
///   - `description`: byte array of the description of the kernel module.
///   - `license`: byte array of the license of the kernel module (required).
///   - `alias`: byte array of alias name of the kernel module.
///   - `alias_rtnl_link`: byte array of the `rtnl_link_alias` of the kernel module (mutually exclusive with `alias`).
///   - `params`: parameters for the kernel module, as described below.
///
/// # Supported parameter types
///
///   - `bool`: Corresponds to C `bool` param type.
///   - `i8`: No equivalent C param type.
///   - `u8`: Corresponds to C `char` param type.
///   - `i16`: Corresponds to C `short` param type.
///   - `u16`: Corresponds to C `ushort` param type.
///   - `i32`: Corresponds to C `int` param type.
///   - `u32`: Corresponds to C `uint` param type.
///   - `i64`: No equivalent C param type.
///   - `u64`: Corresponds to C `ullong` param type.
///   - `isize`: No equivalent C param type.
///   - `usize`: No equivalent C param type.
///   - `str`: Corresponds to C `charp` param type. Reading returns a byte slice.
///   - `ArrayParam<T,N>`: Corresponds to C parameters created using `module_param_array`. An array
///     of `T`'s of length at **most** `N`.
///
/// `invbool` is unsupported: it was only ever used in a few modules.
/// Consider using a `bool` and inverting the logic instead.
#[proc_macro]
pub fn module(ts: TokenStream) -> TokenStream {
    module::module(ts)
}

/// Declares a kernel module that exposes a single misc device.
///
/// The `type` argument should be a type which implements the [`FileOpener`] trait. Also accepts
/// various forms of kernel metadata.
///
/// C header: [`include/linux/moduleparam.h`](../../../include/linux/moduleparam.h)
///
/// [`FileOpener`]: ../kernel/file_operations/trait.FileOpener.html
///
/// # Examples
///
/// ```ignore
/// use kernel::prelude::*;
///
/// module_misc_device! {
///     type: MyFile,
///     name: b"my_miscdev_kernel_module",
///     author: b"Rust for Linux Contributors",
///     description: b"My very own misc device kernel module!",
///     license: b"GPL v2",
/// }
///
/// #[derive(Default)]
/// struct MyFile;
///
/// impl kernel::file_operations::FileOperations for MyFile {
///     kernel::declare_file_operations!();
/// }
/// ```
#[proc_macro]
pub fn module_misc_device(ts: TokenStream) -> TokenStream {
    module::module_misc_device(ts)
}

/// Declares a kernel module that implements a file system.
///
/// The `type` argument should be a type which implements the [`FileSystem`] trait.
///
/// C header: [`include/linux/moduleparam.h`](../../../include/linux/moduleparam.h)
///
/// [`FileSystem`]: ../kernel/fs/trait.FileSystem.html
///
/// # Examples
///
/// ```ignore
/// use kernel::prelude::*;
///
/// module! {
///    type: MyFs,
///    name: b"my_fs",
///    author: b"Rust for Linux Contributors",
///    description: b"My very own fs",
///    license: b"GPL v2",
/// }
/// }
///
/// #[derive(Default)]
/// struct MyFs;
///
/// impl kernel::fs::FileSystem for MyFile {
///     <implement functions as needed here>
/// }
/// ```
#[proc_macro]
pub fn module_fs(ts: TokenStream) -> TokenStream {
    module::module_fs(ts)
}
