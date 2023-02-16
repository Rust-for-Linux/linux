// SPDX-License-Identifier: GPL-2.0

//! DMA fence abstraction.
//!
//! C header: [`include/linux/dma_fence.h`](../../include/linux/dma_fence.h)

use crate::{
    bindings,
    error::{to_result, Result},
    prelude::*,
    sync::LockClassKey,
    types::Opaque,
};
use core::fmt::Write;
use core::ops::{Deref, DerefMut};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU64, Ordering};

/// Any kind of DMA Fence Object
///
/// # Invariants
/// raw() returns a valid pointer to a dma_fence and we own a reference to it.
pub trait RawDmaFence: crate::private::Sealed {
    /// Returns the raw `struct dma_fence` pointer.
    fn raw(&self) -> *mut bindings::dma_fence;

    /// Returns the raw `struct dma_fence` pointer and consumes the object.
    ///
    /// The caller is responsible for dropping the reference.
    fn into_raw(self) -> *mut bindings::dma_fence
    where
        Self: Sized,
    {
        let ptr = self.raw();
        core::mem::forget(self);
        ptr
    }

    /// Advances this fence to the chain node which will signal this sequence number.
    /// If no sequence number is provided, this returns `self` again.
    fn chain_find_seqno(self, seqno: u64) -> Result<Fence>
    where
        Self: Sized,
    {
        let mut ptr = self.into_raw();

        // SAFETY: This will safely fail if this DmaFence is not a chain.
        // `ptr` is valid per the type invariant.
        let ret = unsafe { bindings::dma_fence_chain_find_seqno(&mut ptr, seqno) };

        if ret != 0 {
            // SAFETY: This is either an owned reference or NULL, dma_fence_put can handle both.
            unsafe { bindings::dma_fence_put(ptr) };
            Err(Error::from_kernel_errno(ret))
        } else if ptr.is_null() {
            Err(EINVAL) // When can this happen?
        } else {
            // SAFETY: ptr is valid and non-NULL as checked above.
            Ok(unsafe { Fence::from_raw(ptr) })
        }
    }

    /// Signal completion of this fence
    fn signal(&self) -> Result {
        to_result(unsafe { bindings::dma_fence_signal(self.raw()) })
    }

    /// Set the error flag on this fence
    fn set_error(&self, err: Error) {
        unsafe { bindings::dma_fence_set_error(self.raw(), err.to_kernel_errno()) };
    }
}

/// A generic DMA Fence Object
///
/// # Invariants
/// ptr is a valid pointer to a dma_fence and we own a reference to it.
pub struct Fence {
    ptr: *mut bindings::dma_fence,
}

impl Fence {
    /// Create a new Fence object from a raw pointer to a dma_fence.
    ///
    /// # Safety
    /// The caller must own a reference to the dma_fence, which is transferred to the new object.
    pub(crate) unsafe fn from_raw(ptr: *mut bindings::dma_fence) -> Fence {
        Fence { ptr }
    }

    /// Create a new Fence object from a raw pointer to a dma_fence.
    ///
    /// # Safety
    /// Takes a borrowed reference to the dma_fence, and increments the reference count.
    pub(crate) unsafe fn get_raw(ptr: *mut bindings::dma_fence) -> Fence {
        // SAFETY: Pointer is valid per the safety contract
        unsafe { bindings::dma_fence_get(ptr) };
        Fence { ptr }
    }

    /// Create a new Fence object from a RawDmaFence.
    pub fn from_fence(fence: &dyn RawDmaFence) -> Fence {
        // SAFETY: Pointer is valid per the RawDmaFence contract
        unsafe { Self::get_raw(fence.raw()) }
    }
}

impl crate::private::Sealed for Fence {}

impl RawDmaFence for Fence {
    fn raw(&self) -> *mut bindings::dma_fence {
        self.ptr
    }
}

impl Drop for Fence {
    fn drop(&mut self) {
        // SAFETY: We own a reference to this syncobj.
        unsafe { bindings::dma_fence_put(self.ptr) };
    }
}

impl Clone for Fence {
    fn clone(&self) -> Self {
        // SAFETY: `ptr` is valid per the type invariant and we own a reference to it.
        unsafe {
            bindings::dma_fence_get(self.ptr);
            Self::from_raw(self.ptr)
        }
    }
}

unsafe impl Sync for Fence {}
unsafe impl Send for Fence {}

/// Trait which must be implemented by driver-specific fence objects.
#[vtable]
pub trait FenceOps: Sized + Send + Sync {
    /// True if this dma_fence implementation uses 64bit seqno, false otherwise.
    const USE_64BIT_SEQNO: bool;

    /// Returns the driver name. This is a callback to allow drivers to compute the name at
    /// runtime, without having it to store permanently for each fence, or build a cache of
    /// some sort.
    fn get_driver_name<'a>(self: &'a FenceObject<Self>) -> &'a CStr;

    /// Return the name of the context this fence belongs to. This is a callback to allow drivers
    /// to compute the name at runtime, without having it to store permanently for each fence, or
    /// build a cache of some sort.
    fn get_timeline_name<'a>(self: &'a FenceObject<Self>) -> &'a CStr;

    /// Enable software signaling of fence.
    fn enable_signaling(self: &FenceObject<Self>) -> bool {
        false
    }

    /// Peek whether the fence is signaled, as a fastpath optimization for e.g. dma_fence_wait() or
    /// dma_fence_add_callback().
    fn signaled(self: &FenceObject<Self>) -> bool {
        false
    }

    /// Callback to fill in free-form debug info specific to this fence, like the sequence number.
    fn fence_value_str(self: &FenceObject<Self>, _output: &mut dyn Write) {}

    /// Fills in the current value of the timeline as a string, like the sequence number. Note that
    /// the specific fence passed to this function should not matter, drivers should only use it to
    /// look up the corresponding timeline structures.
    fn timeline_value_str(self: &FenceObject<Self>, _output: &mut dyn Write) {}
}

unsafe extern "C" fn get_driver_name_cb<T: FenceOps>(
    fence: *mut bindings::dma_fence,
) -> *const core::ffi::c_char {
    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::get_driver_name(unsafe { &mut *p }).as_char_ptr()
}

unsafe extern "C" fn get_timeline_name_cb<T: FenceOps>(
    fence: *mut bindings::dma_fence,
) -> *const core::ffi::c_char {
    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::get_timeline_name(unsafe { &mut *p }).as_char_ptr()
}

unsafe extern "C" fn enable_signaling_cb<T: FenceOps>(fence: *mut bindings::dma_fence) -> bool {
    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::enable_signaling(unsafe { &mut *p })
}

unsafe extern "C" fn signaled_cb<T: FenceOps>(fence: *mut bindings::dma_fence) -> bool {
    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::signaled(unsafe { &mut *p })
}

unsafe extern "C" fn release_cb<T: FenceOps>(fence: *mut bindings::dma_fence) {
    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: p is never used after this
    unsafe {
        core::ptr::drop_in_place(&mut (*p).inner);
    }

    // SAFETY: All of our fences are allocated using kmalloc, so this is safe.
    unsafe { bindings::dma_fence_free(fence) };
}

unsafe extern "C" fn fence_value_str_cb<T: FenceOps>(
    fence: *mut bindings::dma_fence,
    string: *mut core::ffi::c_char,
    size: core::ffi::c_int,
) {
    let size: usize = size.try_into().unwrap_or(0);

    if size == 0 {
        return;
    }

    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for the validity of string/size
    let mut f = unsafe { crate::str::Formatter::from_buffer(string as *mut _, size) };

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::fence_value_str(unsafe { &mut *p }, &mut f);
    let _ = f.write_str("\0");

    // SAFETY: `size` is at least 1 per the check above
    unsafe { *string.add(size - 1) = 0 };
}

unsafe extern "C" fn timeline_value_str_cb<T: FenceOps>(
    fence: *mut bindings::dma_fence,
    string: *mut core::ffi::c_char,
    size: core::ffi::c_int,
) {
    let size: usize = size.try_into().unwrap_or(0);

    if size == 0 {
        return;
    }

    // SAFETY: All of our fences are FenceObject<T>.
    let p = crate::container_of!(fence, FenceObject<T>, fence) as *mut FenceObject<T>;

    // SAFETY: The caller is responsible for the validity of string/size
    let mut f = unsafe { crate::str::Formatter::from_buffer(string as *mut _, size) };

    // SAFETY: The caller is responsible for passing a valid dma_fence subtype
    T::timeline_value_str(unsafe { &mut *p }, &mut f);
    let _ = f.write_str("\0");

    // SAFETY: `size` is at least 1 per the check above
    unsafe { *string.add(size - 1) = 0 };
}

// Allow FenceObject<Self> to be used as a self argument, for ergonomics
impl<T: FenceOps> core::ops::Receiver for FenceObject<T> {}

/// A driver-specific DMA Fence Object
///
/// # Invariants
/// ptr is a valid pointer to a dma_fence and we own a reference to it.
#[repr(C)]
pub struct FenceObject<T: FenceOps> {
    fence: bindings::dma_fence,
    lock: Opaque<bindings::spinlock>,
    inner: T,
}

impl<T: FenceOps> FenceObject<T> {
    const SIZE: usize = core::mem::size_of::<Self>();

    const VTABLE: bindings::dma_fence_ops = bindings::dma_fence_ops {
        use_64bit_seqno: T::USE_64BIT_SEQNO,
        get_driver_name: Some(get_driver_name_cb::<T>),
        get_timeline_name: Some(get_timeline_name_cb::<T>),
        enable_signaling: if T::HAS_ENABLE_SIGNALING {
            Some(enable_signaling_cb::<T>)
        } else {
            None
        },
        signaled: if T::HAS_SIGNALED {
            Some(signaled_cb::<T>)
        } else {
            None
        },
        wait: None, // Deprecated
        release: Some(release_cb::<T>),
        fence_value_str: if T::HAS_FENCE_VALUE_STR {
            Some(fence_value_str_cb::<T>)
        } else {
            None
        },
        timeline_value_str: if T::HAS_TIMELINE_VALUE_STR {
            Some(timeline_value_str_cb::<T>)
        } else {
            None
        },
    };
}

impl<T: FenceOps> Deref for FenceObject<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: FenceOps> DerefMut for FenceObject<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: FenceOps> crate::private::Sealed for FenceObject<T> {}
impl<T: FenceOps> RawDmaFence for FenceObject<T> {
    fn raw(&self) -> *mut bindings::dma_fence {
        &self.fence as *const _ as *mut _
    }
}

/// A unique reference to a driver-specific fence object
pub struct UniqueFence<T: FenceOps>(*mut FenceObject<T>);

impl<T: FenceOps> Deref for UniqueFence<T> {
    type Target = FenceObject<T>;

    fn deref(&self) -> &FenceObject<T> {
        unsafe { &*self.0 }
    }
}

impl<T: FenceOps> DerefMut for UniqueFence<T> {
    fn deref_mut(&mut self) -> &mut FenceObject<T> {
        unsafe { &mut *self.0 }
    }
}

impl<T: FenceOps> crate::private::Sealed for UniqueFence<T> {}
impl<T: FenceOps> RawDmaFence for UniqueFence<T> {
    fn raw(&self) -> *mut bindings::dma_fence {
        unsafe { addr_of_mut!((*self.0).fence) }
    }
}

impl<T: FenceOps> From<UniqueFence<T>> for UserFence<T> {
    fn from(value: UniqueFence<T>) -> Self {
        let ptr = value.0;
        core::mem::forget(value);

        UserFence(ptr)
    }
}

impl<T: FenceOps> Drop for UniqueFence<T> {
    fn drop(&mut self) {
        // SAFETY: We own a reference to this fence.
        unsafe { bindings::dma_fence_put(self.raw()) };
    }
}

unsafe impl<T: FenceOps> Sync for UniqueFence<T> {}
unsafe impl<T: FenceOps> Send for UniqueFence<T> {}

/// A shared reference to a driver-specific fence object
pub struct UserFence<T: FenceOps>(*mut FenceObject<T>);

impl<T: FenceOps> Deref for UserFence<T> {
    type Target = FenceObject<T>;

    fn deref(&self) -> &FenceObject<T> {
        unsafe { &*self.0 }
    }
}

impl<T: FenceOps> Clone for UserFence<T> {
    fn clone(&self) -> Self {
        // SAFETY: `ptr` is valid per the type invariant and we own a reference to it.
        unsafe {
            bindings::dma_fence_get(self.raw());
            Self(self.0)
        }
    }
}

impl<T: FenceOps> crate::private::Sealed for UserFence<T> {}
impl<T: FenceOps> RawDmaFence for UserFence<T> {
    fn raw(&self) -> *mut bindings::dma_fence {
        unsafe { addr_of_mut!((*self.0).fence) }
    }
}

impl<T: FenceOps> Drop for UserFence<T> {
    fn drop(&mut self) {
        // SAFETY: We own a reference to this fence.
        unsafe { bindings::dma_fence_put(self.raw()) };
    }
}

unsafe impl<T: FenceOps> Sync for UserFence<T> {}
unsafe impl<T: FenceOps> Send for UserFence<T> {}

/// An array of fence contexts, out of which fences can be created.
pub struct FenceContexts {
    start: u64,
    count: u32,
    seqnos: Vec<AtomicU64>,
    lock_name: &'static CStr,
    lock_key: &'static LockClassKey,
}

impl FenceContexts {
    /// Create a new set of fence contexts.
    pub fn new(
        count: u32,
        name: &'static CStr,
        key: &'static LockClassKey,
    ) -> Result<FenceContexts> {
        let mut seqnos: Vec<AtomicU64> = Vec::new();

        seqnos.try_reserve(count as usize)?;

        for _ in 0..count {
            seqnos.try_push(Default::default())?;
        }

        let start = unsafe { bindings::dma_fence_context_alloc(count as core::ffi::c_uint) };

        Ok(FenceContexts {
            start,
            count,
            seqnos,
            lock_name: name,
            lock_key: key,
        })
    }

    /// Create a new fence in a given context index.
    pub fn new_fence<T: FenceOps>(&self, context: u32, inner: T) -> Result<UniqueFence<T>> {
        if context > self.count {
            return Err(EINVAL);
        }

        let p = unsafe {
            bindings::krealloc(
                core::ptr::null_mut(),
                FenceObject::<T>::SIZE,
                bindings::GFP_KERNEL | bindings::__GFP_ZERO,
            ) as *mut FenceObject<T>
        };

        if p.is_null() {
            return Err(ENOMEM);
        }

        let seqno = self.seqnos[context as usize].fetch_add(1, Ordering::Relaxed);

        // SAFETY: The pointer is valid, so pointers to members are too.
        // After this, all fields are initialized.
        unsafe {
            addr_of_mut!((*p).inner).write(inner);
            bindings::__spin_lock_init(
                addr_of_mut!((*p).lock) as *mut _,
                self.lock_name.as_char_ptr(),
                self.lock_key.get(),
            );
            bindings::dma_fence_init(
                addr_of_mut!((*p).fence),
                &FenceObject::<T>::VTABLE,
                addr_of_mut!((*p).lock) as *mut _,
                self.start + context as u64,
                seqno,
            );
        };

        Ok(UniqueFence(p))
    }
}

/// A DMA Fence Chain Object
///
/// # Invariants
/// ptr is a valid pointer to a dma_fence_chain which we own.
pub struct FenceChain {
    ptr: *mut bindings::dma_fence_chain,
}

impl FenceChain {
    /// Create a new DmaFenceChain object.
    pub fn new() -> Result<Self> {
        // SAFETY: This function is safe to call and takes no arguments.
        let ptr = unsafe { bindings::dma_fence_chain_alloc() };

        if ptr.is_null() {
            Err(ENOMEM)
        } else {
            Ok(FenceChain { ptr })
        }
    }

    /// Convert the DmaFenceChain into the underlying raw pointer.
    ///
    /// This assumes the caller will take ownership of the object.
    pub(crate) fn into_raw(self) -> *mut bindings::dma_fence_chain {
        let ptr = self.ptr;
        core::mem::forget(self);
        ptr
    }
}

impl Drop for FenceChain {
    fn drop(&mut self) {
        // SAFETY: We own this dma_fence_chain.
        unsafe { bindings::dma_fence_chain_free(self.ptr) };
    }
}
