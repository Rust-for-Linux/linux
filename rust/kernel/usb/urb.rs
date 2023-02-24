// SPDX-License-Identifier: GPL-2.0

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::{marker::PhantomData, ops::Deref, pin::Pin, ptr::NonNull};

use crate::{
    bindings,
    error::{code::*, Result},
    gfp_t,
    sync::{Arc, ArcBorrow},
    to_result,
    types::{ForeignOwnable, ScopeGuard},
    GFP_ATOMIC, GFP_KERNEL,
};

use super::Device;

/// URB transfer flags macros reexports and casted to [`u32`] intended for the
/// [`transfer_flags`](Urb::transfer_flags) field of [`Urb`].
pub mod transfer_flags {
    #![allow(missing_docs)]
    pub const URB_SHORT_NOT_OK: u32 = bindings::URB_SHORT_NOT_OK;
    pub const URB_ISO_ASAP: u32 = bindings::URB_ISO_ASAP;
    pub const URB_NO_TRANSFER_DMA_MAP: u32 = bindings::URB_NO_TRANSFER_DMA_MAP;
    pub const URB_ZERO_PACKET: u32 = bindings::URB_ZERO_PACKET;
    pub const URB_NO_INTERRUPT: u32 = bindings::URB_NO_INTERRUPT;
    pub const URB_FREE_BUFFER: u32 = bindings::URB_FREE_BUFFER;
}

/// An URB transfer buffer.
pub trait Transfer {
    /// Type of values borrowed between calls to [`Transfer::into_data`] and
    /// [`Transfer::from_data`].
    type Borrowed<'a>;

    /// Assembles a transfer buffer from a pointer to data and its size.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous call to [`Transfer::into_data`].
    ///
    /// # Errors
    ///
    /// Returns [`EINVAL`] if either the data was null or the given size does comply with the
    /// requirements imposed by the type, like being shorter than the type's size.
    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Self>
    where
        Self: Sized;

    /// Assembles a transfer buffer from a pointer to data and its size.
    ///
    /// # Safety
    ///
    /// `data` must have been returned by a previous call to [`Transfer::into_data`].
    /// Additionally, [`Transfer::from_data`] can only be called after *all* values
    /// returned by [`Transfer::borrow`] have been dropped.
    ///
    /// # Errors
    ///
    /// Returns [`EINVAL`] if either the data was null or the given size does comply with the
    /// requirements imposed by the type, like being shorter than the type's size.
    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<Self::Borrowed<'a>>
    where
        Self: Sized;

    /// Returns a mutably borrowed value.
    ///
    /// # Safety
    ///
    /// The passed pointer must come from a previous to [`Transfer::into_data`], and no
    /// other concurrent users of the pointer (except the ones derived from the returned value) run
    /// at least until the returned [`ScopeGuard`] is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`EINVAL`] if either the data was null or the given size does comply with the
    /// requirements imposed by the type, like being shorter than the type's size.
    unsafe fn borrow_mut<T: Transfer>(
        data: *mut core::ffi::c_void,
        size: usize,
    ) -> Result<ScopeGuard<T, fn(T)>> {
        // SAFETY: The safety requirements ensure that `ptr` came from a previous call to
        // `into_data`.
        Ok(ScopeGuard::new_with_data(
            unsafe { T::from_data(data, size)? },
            |d| {
                d.into_data();
            },
        ))
    }

    /// Returns the pointer to the underlying buffer data.
    fn into_data(self) -> *mut core::ffi::c_void;

    /// Returns the buffer length.
    fn transfer_len(&self) -> usize;
}

impl<T: 'static> Transfer for &mut [T] {
    type Borrowed<'a> = &'a [T];

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Self> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements of this function ensure that `ptr` comes from a previous
            // call to `Self::into_data`.
            Ok(unsafe { core::slice::from_raw_parts_mut(data.cast(), size) })
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<&'a [T]> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements for this function ensure that the object is still alive,
            // so it is safe to build an slice from the raw pointer.
            // The safety requirements of `from_data` also ensure that the object remains alive for
            // the lifetime of the returned value.
            Ok(unsafe { core::slice::from_raw_parts(data as *const _, size) })
        }
    }

    fn into_data(self) -> *mut core::ffi::c_void {
        self.as_mut_ptr().cast()
    }

    fn transfer_len(&self) -> usize {
        self.len()
    }
}

impl<T: 'static> Transfer for &mut T {
    type Borrowed<'a> = &'a T;

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Self> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements of this function ensure that `ptr` comes from a previous
            // call to `Self::into_data`.
            Ok(unsafe { &mut *(data.cast()) })
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<&'a T> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements for this function ensure that the object is still alive,
            // so it is safe to dereference the raw pointer.
            // The safety requirements of `from_data` also ensure that the object remains alive for
            // the lifetime of the returned value.
            Ok(unsafe { &*(data as *const _) })
        }
    }

    fn into_data(self) -> *mut core::ffi::c_void {
        (self as *mut T).cast()
    }

    fn transfer_len(&self) -> usize {
        core::mem::size_of::<T>()
    }
}

impl<T: 'static> Transfer for Vec<T> {
    type Borrowed<'a> = &'a [T];

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Vec<T>> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements of this function ensure that `ptr` comes from a previous
            // call to `Self::into_data`.
            Ok(unsafe { Vec::from_raw_parts(data.cast(), size, size) })
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<&'a [T]> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements for this function ensure that the object is still alive,
            // so it is safe to build an slice from the raw pointer.
            // The safety requirements of `from_data` also ensure that the object remains alive for
            // the lifetime of the returned value.
            Ok(unsafe { core::slice::from_raw_parts(data as *const _, size) })
        }
    }

    fn into_data(mut self) -> *mut core::ffi::c_void {
        self.as_mut_ptr().cast()
    }

    fn transfer_len(&self) -> usize {
        self.len()
    }
}

impl<T: 'static> Transfer for Box<T> {
    type Borrowed<'a> = &'a T;

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Box<T>> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements of this function ensure that `ptr` comes from a previous
            // call to `Self::into_data`.
            Ok(unsafe { Box::from_raw(data.cast()) })
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<&'a T> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements for this function ensure that the object is still alive,
            // so it is safe to dereference the raw pointer.
            // The safety requirements of `from_data` also ensure that the object remains alive for
            // the lifetime of the returned value.
            Ok(unsafe { &*data.cast() })
        }
    }

    fn into_data(self) -> *mut core::ffi::c_void {
        Box::into_raw(self).cast()
    }

    fn transfer_len(&self) -> usize {
        core::mem::size_of::<T>()
    }
}

impl<T: 'static> Transfer for Arc<T> {
    type Borrowed<'a> = ArcBorrow<'a, T>;

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Arc<T>> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: By the safety requirement of this function, we know that `ptr` came from
            // a previous call to `Arc::into_data`, which guarantees that `ptr` is valid and
            // holds a reference count increment that is transferrable to us.
            unsafe { Ok(Arc::from_foreign(data)) }
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<ArcBorrow<'a, T>> {
        if data.is_null() || size < core::mem::size_of::<T>() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements of `from_data` ensure that the object remains alive
            // for the lifetime of the returned value. Additionally, the safety requirements of
            // `Transfer::borrow_mut` ensure that no new mutable references are created.
            unsafe { Ok(<Arc<T> as ForeignOwnable>::borrow(data)) }
        }
    }

    fn into_data(self) -> *mut core::ffi::c_void {
        Arc::into_foreign(self).cast_mut()
    }

    fn transfer_len(&self) -> usize {
        core::mem::size_of::<T>()
    }
}

impl<T: Transfer + Deref> Transfer for Pin<T>
where
    <T as Deref>::Target: Transfer,
{
    type Borrowed<'a> = T::Borrowed<'a>;

    unsafe fn from_data(data: *mut core::ffi::c_void, size: usize) -> Result<Pin<T>> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The object was originally pinned.
            // The passed pointer comes from a previous call to `T::into_data`.
            Ok(unsafe { Pin::new_unchecked(T::from_data(data.cast(), size)?) })
        }
    }

    unsafe fn borrow<'a>(data: *mut core::ffi::c_void, size: usize) -> Result<Self::Borrowed<'a>> {
        if data.is_null() {
            Err(EINVAL)
        } else {
            // SAFETY: The safety requirements for this function are the same as the ones for
            // `T::borrow`.
            unsafe { T::borrow(data, size) }
        }
    }

    fn into_data(self) -> *mut core::ffi::c_void {
        // SAFETY: We continue to treat the pointer as pinned by returning just a pointer to it to
        // the caller.
        let inner = unsafe { Pin::into_inner_unchecked(self) };
        inner.into_data()
    }

    fn transfer_len(&self) -> usize {
        self.as_ref().transfer_len()
    }
}

/// URB completion handler.
pub trait Completion<T: Transfer, C: ForeignOwnable + Send + Sync> {
    /// URB completion routine.
    ///
    /// Made when the action of the URB has been successfully completed or cancelled.
    fn complete(urb: Urb<T, C>);
}

/// Setup data for a USB device control request.
#[derive(Default, Copy, Clone, PartialEq)]
#[repr(C, packed)]
pub struct ControlRequest {
    /// Matches the USB bmRequestType field.
    pub b_request_type: u8,
    /// Matches the USB bRequest field.
    pub b_request: u8,
    /// Matches the USB wValue field (le16 byte order).
    pub w_value: bindings::__le16,
    /// Matches the USB wIndex field (le16 byte order).
    pub w_index: bindings::__le16,
    /// Matches the USB wLength field (le16 byte order).
    pub w_length: bindings::__le16,
}

impl ControlRequest {
    /// Allocates a control request with `GFP_KERNEL` and boxes it.
    ///
    /// See [`try_boxed_flagged`].
    #[inline]
    pub fn try_boxed() -> Result<Box<Self>> {
        Self::try_boxed_flagged(GFP_KERNEL)
    }

    /// Allocates a control request with `GFP_ATOMIC` and boxes it.
    ///
    /// See [`try_boxed_flagged`].
    #[inline]
    pub fn try_boxed_atomic() -> Result<Box<Self>> {
        Self::try_boxed_flagged(GFP_ATOMIC)
    }

    /// Allocates a control request with the provided `gfp_t` flag combination and boxes it.
    ///
    /// # Errors
    ///
    /// An allocation error is returned upon failure.
    pub fn try_boxed_flagged(flags: gfp_t) -> Result<Box<Self>> {
        // SAFETY: FFI call.
        let cr =
            unsafe { bindings::krealloc(core::ptr::null(), core::mem::size_of::<Self>(), flags) };
        if cr.is_null() {
            Err(ENOMEM)
        } else {
            Ok(unsafe { Box::from_raw(cr.cast()) })
        }
    }
}

unsafe extern "C" fn complete_callback<
    R: Completion<T, C>,
    C: ForeignOwnable + Send + Sync,
    T: Transfer,
>(
    ptr: *mut bindings::urb,
) {
    // SAFETY: The pointer `ptr` is non-null and valid for the duration of the callback.
    unsafe {
        let mut urb = Urb::from_ptr(ptr);
        urb.get();
        R::complete(urb);
    }
}

/// An USB request block (URB).
///
/// # Invariants
///
/// The field `ptr` is non-null and valid for the lifetime of the object.
pub struct Urb<T: Transfer, C: ForeignOwnable + Send + Sync> {
    ptr: *mut bindings::urb,
    _transfer: PhantomData<T>,
    _context: PhantomData<C>,
}

impl<T: Transfer, C: ForeignOwnable + Send + Sync> Urb<T, C> {
    /// Creates an URB for a USB driver to use with [`GFP_KERNEL`] memory type.
    ///
    /// See [`try_new_flagged`].
    #[inline]
    pub fn try_new(pkts: i32) -> Result<Self> {
        Self::try_new_flagged(pkts, GFP_KERNEL)
    }

    /// Creates an URB for a USB driver to use with [`GFP_ATOMIC`] memory type.
    ///
    /// See [`try_new_flagged`].
    #[inline]
    pub fn try_new_atomic(pkts: i32) -> Result<Self> {
        Self::try_new_flagged(pkts, GFP_ATOMIC)
    }

    /// Creates an URB for a USB driver to use with the provided [`gfp_t`] flag combination.
    ///
    /// If the driver wants to use this urb for interrupt, control, or bulk endpoints, pass `0` as
    /// the number of iso packets.
    ///
    /// # Errors
    ///
    /// An allocation error is returned upon failure.
    #[inline]
    pub fn try_new_flagged(pkts: i32, flags: gfp_t) -> Result<Self> {
        // SAFETY: FFI call.
        let urb = unsafe { Self::from_ptr(bindings::usb_alloc_urb(pkts, flags)) };
        if urb.ptr.is_null() {
            Err(ENOMEM)
        } else {
            Ok(urb)
        }
    }

    /// Creates an URB from a raw pointer.
    ///
    /// # Safety
    ///
    /// It requires that a valid pointer to [`bindings::urb`] has to be passed.
    /// Must remain as valid during the life time of the instance as well.
    #[inline]
    unsafe fn from_ptr(ptr: *mut bindings::urb) -> Self {
        Self {
            ptr,
            _transfer: PhantomData,
            _context: PhantomData,
        }
    }

    /// Returns the raw `struct urb` related to `self`.
    pub fn raw(&self) -> *mut bindings::urb {
        self.ptr
    }

    /// Current status of the URB.
    pub fn status(&self) -> Result {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { to_result((*self.ptr).status) }
    }

    /// Increments the reference count of the URB.
    ///
    /// # Safety
    ///
    /// The URB must not been freed prior to calling this function.
    #[inline]
    pub fn get(&mut self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            self.ptr = bindings::usb_get_urb(self.ptr);
        }
    }

    /// Gets the URB transfer flags.
    #[inline]
    pub fn transfer_flags(&self) -> u32 {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { (*self.ptr).transfer_flags }
    }

    /// Sets the URB transfer flags.
    #[inline]
    pub fn set_transfer_flags(&mut self, flags: u32) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            (*self.ptr).transfer_flags = flags;
        }
    }

    /// Fills a control request with the URB.
    #[inline]
    pub fn fill_control<R: Completion<T, C>>(
        &mut self,
        dev: &Device,
        pipe: u32,
        setup_pkt: Box<ControlRequest>,
        transfer: Option<T>,
        ctx: Option<C>,
    ) {
        let len = transfer
            .as_ref()
            .map_or(0, T::transfer_len)
            .try_into()
            .unwrap_or(i32::MAX);
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_fill_control_urb(
                self.ptr,
                dev.ptr,
                pipe,
                (Box::into_raw(setup_pkt)).cast::<u8>(),
                transfer.map_or(core::ptr::null_mut(), T::into_data),
                len,
                Some(complete_callback::<R, C, T>),
                ctx.map_or(core::ptr::null_mut(), |c| c.into_foreign() as *mut _),
            );
        };
    }

    /// Fills a bulk request with the URB.
    #[inline]
    pub fn fill_bulk<R: Completion<T, C>>(
        &mut self,
        dev: &Device,
        pipe: u32,
        transfer: Option<T>,
        ctx: Option<C>,
    ) {
        let len = transfer
            .as_ref()
            .map_or(0, T::transfer_len)
            .try_into()
            .unwrap_or(i32::MAX);
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_fill_bulk_urb(
                self.ptr,
                dev.ptr,
                pipe,
                transfer.map_or(core::ptr::null_mut(), T::into_data),
                len,
                Some(complete_callback::<R, C, T>),
                ctx.map_or(core::ptr::null_mut(), |c| c.into_foreign() as *mut _),
            );
        };
    }

    /// Fills an interrupt request with the URB.
    #[inline]
    pub fn fill_int<R: Completion<T, C>>(
        &mut self,
        dev: &Device,
        pipe: u32,
        transfer: Option<T>,
        ctx: Option<C>,
        interval: i32,
    ) {
        let len = transfer
            .as_ref()
            .map_or(0, T::transfer_len)
            .try_into()
            .unwrap_or(i32::MAX);
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_fill_int_urb(
                self.ptr,
                dev.ptr,
                pipe,
                transfer.map_or(core::ptr::null_mut(), T::into_data),
                len,
                Some(complete_callback::<R, C, T>),
                ctx.map_or(core::ptr::null_mut(), |c| c.into_foreign() as *mut _),
                interval,
            );
        };
    }

    /// Returns a reference to the context of the URB.
    pub fn context<'a>(&self) -> Option<C::Borrowed<'a>> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { NonNull::new((*self.ptr).context).map(|c| C::borrow(c.as_ptr())) }
    }

    /// Returns a mutable reference to the context of the URB.
    pub fn context_mut(&mut self) -> Option<ScopeGuard<C, fn(C)>> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { NonNull::new((*self.ptr).context).map(|c| C::borrow_mut(c.as_ptr())) }
    }

    /// Takes ownership over the context of the URB.
    pub fn take_context(&mut self) -> Option<C> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            let ctx = NonNull::new((*self.ptr).context).map(|c| C::from_foreign(c.as_ptr()));
            (*self.ptr).context = core::ptr::null_mut();
            ctx
        }
    }

    /// Returns a reference to the transfer buffer of the URB.
    pub fn borrow_transfer<'a>(&self) -> Result<T::Borrowed<'a>> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            T::borrow(
                (*self.ptr).transfer_buffer,
                (*self.ptr).transfer_buffer_length as usize,
            )
        }
    }

    /// Returns a mutable reference to the transfer buffer of the URB.
    pub fn borrow_transfer_mut(&mut self) -> Result<ScopeGuard<T, fn(T)>> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            T::borrow_mut(
                (*self.ptr).transfer_buffer,
                (*self.ptr).transfer_buffer_length as usize,
            )
        }
    }

    /// Takes ownership over the transfer buffer of the URB.
    pub fn take_transfer(&mut self) -> Result<T> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            let transfer = T::from_data(
                (*self.ptr).transfer_buffer,
                (*self.ptr).transfer_buffer_length as usize,
            );
            (*self.ptr).transfer_buffer = core::ptr::null_mut();
            (*self.ptr).transfer_buffer_length = 0;
            transfer
        }
    }

    /// Returns a reference to the setup packet of the URB.
    pub fn setup_packet<'a>(&self) -> Option<&'a ControlRequest> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { NonNull::new((*self.ptr).setup_packet).map(|p| &*(p.as_ptr().cast())) }
    }

    /// Returns a mutable reference to the setup packet of the URB.
    pub fn setup_packet_mut<'a>(&mut self) -> Option<&'a mut ControlRequest> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe { NonNull::new((*self.ptr).setup_packet).map(|p| &mut *(p.as_ptr().cast())) }
    }

    /// Takes ownership over the setup packet of the URB.
    pub fn take_setup_packet(&mut self) -> Option<Box<ControlRequest>> {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        unsafe {
            let setup_pkt =
                NonNull::new((*self.ptr).setup_packet).map(|p| Box::from_raw(p.as_ptr().cast()));
            (*self.ptr).setup_packet = core::ptr::null_mut();
            setup_pkt
        }
    }

    /// Submits the URB for completion.
    #[inline]
    pub fn submit(&mut self, flags: gfp_t) -> Result {
        // SAFETY: FFI call.
        unsafe { to_result(bindings::usb_submit_urb(self.ptr, flags)) }
    }

    /// Kills the URB.
    #[inline]
    pub fn kill(&mut self) {
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_kill_urb(self.ptr);
        }
    }

    /// Poisons the URB.
    #[inline]
    pub fn poison(&mut self) {
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_poison_urb(self.ptr);
        }
    }

    /// Unpoisons the URB.
    #[inline]
    pub fn unpoison(&mut self) {
        // SAFETY: FFI call.
        unsafe {
            bindings::usb_unpoison_urb(self.ptr);
        }
    }
}

impl<T: Transfer, C: ForeignOwnable + Send + Sync> Drop for Urb<T, C> {
    fn drop(&mut self) {
        // SAFETY: By the type invariants, we know that `self.ptr` is non-null and valid.
        if unsafe { bindings::refcount_read(&mut (*self.ptr).kref.refcount) == 0 } {
            // Take ownership of every optional passed to the `Urb::fill_*` routines.
            self.take_setup_packet();
            self.take_context();
            self.take_transfer().ok();
        }
        unsafe { bindings::usb_free_urb(self.ptr) };
    }
}

// SAFETY: `Urb` only holds a pointer to an URB, which is safe to be used from any thread.
unsafe impl<T: Transfer, C: ForeignOwnable + Send + Sync> Send for Urb<T, C> {}

// SAFETY: `Urb` only holds a pointer to an URB, which is safe to be used from any thread.
unsafe impl<T: Transfer, C: ForeignOwnable + Send + Sync> Sync for Urb<T, C> {}
