// SPDX-License-Identifier: GPL-2.0

//! Kernel file systems.
//!
//! C headers: [`include/linux/fs.h`](srctree/include/linux/fs.h)

use crate::{
    alloc::{flags, Allocator, KBox},
    bindings,
    error::code::EINVAL,
    error::{to_result, Error, Result},
    pr_info,
    prelude::Box,
    str::CStr,
    ThisModule,
};
use core::{
    cell::UnsafeCell,
    marker::{PhantomData, PhantomPinned, Send, Sync},
    pin::Pin,
};

pub mod file;
pub use self::file::{File, LocalFile};

mod kiocb;
pub use self::kiocb::Kiocb;

/// A file system type.
pub trait Type {
    /// The name of the file system type.
    const NAME: &'static CStr;
}

/// A file system registration.
#[derive(Default)]
pub struct Registration {
    is_registered: bool,
    fs: UnsafeCell<bindings::file_system_type>,
    _pin: PhantomPinned,
}

impl Registration {
    /// Creates a new file system registration.
    ///
    /// It is not visible or accessible yet. A successful call to [`Registration::register`] needs
    /// to be made before users can mount it.
    pub fn new() -> Self {
        Self {
            is_registered: false,
            fs: UnsafeCell::new(bindings::file_system_type::default()),
            _pin: PhantomPinned,
        }
    }

    pub fn register<T: Type + ?Sized>(self: Pin<&mut Self>, module: &'static ThisModule) -> Result {
        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };

        if this.is_registered {
            return Err(EINVAL);
        }

        this.is_registered = true;
        Ok(())
    }
}

// SAFETY: `Registration` doesn't really provide any `&self` methods, so it is safe to pass
// references to it around.
unsafe impl Sync for Registration {}

// SAFETY: Both registration and unregistration are implemented in C and safe to be performed from
// any thread, so `Registration` is `Send`.
unsafe impl Send for Registration {}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.is_registered {
            pr_info!("Unloading FS module\n");

            // SAFETY: When `is_registered` is `true`, a previous call to `register_filesystem` has
            // succeeded, so it is safe to unregister here.
            // unsafe { bindings::unregister_filesystem(self.fs.get()) };
        }
    }
}

/// Kernel module that exposes a single file system implemented by `T`.
pub struct Module<T: Type> {
    _fs: Pin<KBox<Registration>>,
    _p: PhantomData<T>,
}

impl<T: Type + Sync + Send> crate::Module for Module<T> {
    fn init(module: &'static ThisModule) -> Result<Self> {
        pr_info!("Loading FS module\n");
        let reg = Registration::new();
        let kbox = KBox::<Registration>::new(reg, flags::GFP_KERNEL)?;
        let mut reg = Pin::from(kbox);

        reg.as_mut().register::<T>(module)?;

        Ok(Self {
            _fs: reg,
            _p: PhantomData,
        })
    }
}

#[macro_export]
macro_rules! module_fs {
    (type: $ty:ty, $($rest:tt)*) => {
        struct __FsModuleWrapper($crate::fs::Module<$ty>);
        impl $crate::Module for __FsModuleWrapper {
            fn init(module: &'static $crate::ThisModule)
                -> $crate::error::Result<Self>
            {
                let inner = < $crate::fs::Module<$ty> as $crate::Module >::init(module)?;
                Ok(Self(inner))
            }
        }
        $crate::macros::module! { type: __FsModuleWrapper, $($rest)* }
    };
}
