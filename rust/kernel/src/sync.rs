// SPDX-License-Identifier: GPL-2.0

use core::cell::UnsafeCell;
use core::fmt;
use core::ops::{Deref, DerefMut};

use crate::bindings;

pub struct Mutex<T: ?Sized> {
    lock: bindings::mutex,
    data: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    /// Create a new Mutex
    pub fn new(data: T, _name: &'static str) -> Self {
        let lock = bindings::mutex::default();
        // TODO: write mutex debug traces like .magic.
        // TODO: use name in the debug version

        Self {
            data: UnsafeCell::new(data),
            lock,
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// acquire a lock on the mutex
    /// # unsafe
    /// This is unsafe, as it returns a second lock if one is already help by the current process
    // with CONFIG_DEBUG_LOCK_ALLOW mutex_lock is a macro, which calls mutex_lock_nested(&mutex, 0)
    #[cfg(CONFIG_DEBUG_LOCK_ALLOC)]
    pub unsafe fn lock<'a>(&'a self) -> MutexGuard<'a, T> {
        unsafe {
            bindings::mutex_lock_nested(
                &self.lock as *const bindings::mutex as *mut bindings::mutex,
                0,
            );
        }
        MutexGuard { inner: &self }
    }

    /// acquire a lock on the mutex
    /// # unsafe
    /// This is unsafe, as it returns a second lock if one is already help by the current process
    #[cfg(not(CONFIG_DEBUG_LOCK_ALLOC))]
    pub unsafe fn lock<'a>(&'a self) -> MutexGuard<'a, T> {
        unsafe {
            bindings::mutex_lock(&self.lock as *const bindings::mutex as *mut bindings::mutex);
        }
        MutexGuard { inner: &self }
    }

    /// try to acquire the lock, returns none on failure
    /// # unsafe
    /// This is unsafe, as it returns a second lock if one is already help by the current process
    pub unsafe fn trylock<'a>(&'a self) -> Option<MutexGuard<'a, T>> {
        let ret = unsafe {
            bindings::mutex_trylock(&self.lock as *const bindings::mutex as *mut bindings::mutex)
        };
        if ret == 1 {
            Some(MutexGuard { inner: &self })
        } else {
            None
        }
    }

    /// test if the mutex is locked
    pub fn is_locked(&self) -> bool {
        unsafe {
            bindings::mutex_is_locked(&self.lock as *const bindings::mutex as *mut bindings::mutex)
        }
    }

    fn unlock(&self) {
        unsafe {
            bindings::mutex_unlock(&self.lock as *const bindings::mutex as *mut bindings::mutex);
        }
    }
}

unsafe impl<T: ?Sized> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // kindly borrowed from std::sync::Mutex
        match unsafe { self.trylock() } {
            Some(guard) => f.debug_struct("Mutex").field("data", &guard).finish(),
            None => {
                struct LockedPlaceholder;
                impl fmt::Debug for LockedPlaceholder {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("<locked>")
                    }
                }

                f.debug_struct("Mutex")
                    .field("data", &LockedPlaceholder)
                    .finish()
            }
        }
    }
}

#[must_use]
pub struct MutexGuard<'a, T: ?Sized> {
    inner: &'a Mutex<T>,
}

impl<'a, T: ?Sized> !Send for MutexGuard<'a, T> {}
unsafe impl<'a, T: ?Sized> Sync for MutexGuard<'a, T> {}

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.data.get() }
    }
}

impl<'a, T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.deref(), f)
    }
}

impl<'a, T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.deref(), f)
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.inner.unlock();
    }
}
