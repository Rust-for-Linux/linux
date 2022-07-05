// SPDX-License-Identifier: GPL-2.0

//! Interrupts and interrupt chips.
//!
//! See <https://www.kernel.org/doc/Documentation/core-api/genericirq.rst>.
//!
//! C headers: [`include/linux/irq.h`](../../../../include/linux/irq.h) and
//! [`include/linux/interrupt.h`](../../../../include/linux/interrupt.h).

#![allow(dead_code)]

use crate::{
    bindings,
    error::{from_kernel_result, to_result},
    str::CString,
    types::PointerWrapper,
    Error, Result, ScopeGuard,
};
use core::{fmt, marker::PhantomData, ops::Deref};

/// The type of irq hardware numbers.
pub type HwNumber = bindings::irq_hw_number_t;

/// Wraps the kernel's `struct irq_data`.
///
/// # Invariants
///
/// The pointer `IrqData::ptr` is non-null and valid.
pub struct IrqData {
    ptr: *mut bindings::irq_data,
}

impl IrqData {
    /// Creates a new `IrqData` instance from a raw pointer.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `ptr` is non-null and valid when the function is called, and that
    /// it remains valid for the lifetime of the return [`IrqData`] instance.
    unsafe fn from_ptr(ptr: *mut bindings::irq_data) -> Self {
        // INVARIANTS: By the safety requirements, the instance we're creating satisfies the type
        // invariants.
        Self { ptr }
    }

    /// Returns the hardware irq number.
    pub fn hwirq(&self) -> HwNumber {
        // SAFETY: By the type invariants, it's ok to dereference `ptr`.
        unsafe { (*self.ptr).hwirq }
    }
}

/// Wraps the kernel's `struct irq_data` when it is locked.
///
/// Being locked allows additional operations to be performed on the data.
pub struct LockedIrqData(IrqData);

impl LockedIrqData {
    /// Sets the high-level irq flow handler to the builtin one for level-triggered irqs.
    pub fn set_level_handler(&mut self) {
        // SAFETY: By the type invariants of `self.0`, we know `self.0.ptr` is valid.
        unsafe { bindings::irq_set_handler_locked(self.0.ptr, Some(bindings::handle_level_irq)) };
    }

    /// Sets the high-level irq flow handler to the builtin one for edge-triggered irqs.
    pub fn set_edge_handler(&mut self) {
        // SAFETY: By the type invariants of `self.0`, we know `self.0.ptr` is valid.
        unsafe { bindings::irq_set_handler_locked(self.0.ptr, Some(bindings::handle_edge_irq)) };
    }

    /// Sets the high-level irq flow handler to the builtin one for bad irqs.
    pub fn set_bad_handler(&mut self) {
        // SAFETY: By the type invariants of `self.0`, we know `self.0.ptr` is valid.
        unsafe { bindings::irq_set_handler_locked(self.0.ptr, Some(bindings::handle_bad_irq)) };
    }
}

impl Deref for LockedIrqData {
    type Target = IrqData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extra information returned by some of the [`Chip`] methods on success.
pub enum ExtraResult {
    /// Indicates that the caller (irq core) will update the descriptor state.
    None = bindings::IRQ_SET_MASK_OK as _,

    /// Indicates that the callee (irq chip implementation) already updated the descriptor state.
    NoCopy = bindings::IRQ_SET_MASK_OK_NOCOPY as _,

    /// Same as [`ExtraResult::None`] in terms of updating descriptor state. It is used in stacked
    /// irq chips to indicate that descendant chips should be skipped.
    Done = bindings::IRQ_SET_MASK_OK_DONE as _,
}

/// An irq chip.
///
/// It is a trait for the functions defined in [`struct irq_chip`].
///
/// [`struct irq_chip`]: ../../../include/linux/irq.h
pub trait Chip: Sized {
    /// The type of the context data stored in the irq chip and made available on each callback.
    type Data: PointerWrapper;

    /// The methods to use to populate [`struct irq_chip`]. This is typically populated with
    /// [`declare_irq_chip_operations`].
    const TO_USE: ToUse;

    /// Called at the start of a new interrupt.
    fn ack(data: <Self::Data as PointerWrapper>::Borrowed<'_>, irq_data: &IrqData);

    /// Masks an interrupt source.
    fn mask(data: <Self::Data as PointerWrapper>::Borrowed<'_>, irq_data: &IrqData);

    /// Unmasks an interrupt source.
    fn unmask(_data: <Self::Data as PointerWrapper>::Borrowed<'_>, irq_data: &IrqData);

    /// Sets the flow type of an interrupt.
    ///
    /// The flow type is a combination of the constants in [`Type`].
    fn set_type(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _irq_data: &mut LockedIrqData,
        _flow_type: u32,
    ) -> Result<ExtraResult> {
        Ok(ExtraResult::None)
    }

    /// Enables or disables power-management wake-on of an interrupt.
    fn set_wake(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _irq_data: &IrqData,
        _on: bool,
    ) -> Result {
        Ok(())
    }
}

/// Initialises `chip` with the callbacks defined in `T`.
///
/// # Safety
///
/// The caller must ensure that the value stored in the irq chip data is the result of calling
/// [`PointerWrapper::into_pointer] for the [`T::Data`] type.
pub(crate) unsafe fn init_chip<T: Chip>(chip: &mut bindings::irq_chip) {
    chip.irq_ack = Some(irq_ack_callback::<T>);
    chip.irq_mask = Some(irq_mask_callback::<T>);
    chip.irq_unmask = Some(irq_unmask_callback::<T>);

    if T::TO_USE.set_type {
        chip.irq_set_type = Some(irq_set_type_callback::<T>);
    }

    if T::TO_USE.set_wake {
        chip.irq_set_wake = Some(irq_set_wake_callback::<T>);
    }
}

/// Represents which fields of [`struct irq_chip`] should be populated with pointers.
///
/// This is typically populated with the [`declare_irq_chip_operations`] macro.
pub struct ToUse {
    /// The `irq_set_type` field of [`struct irq_chip`].
    pub set_type: bool,

    /// The `irq_set_wake` field of [`struct irq_chip`].
    pub set_wake: bool,
}

/// A constant version where all values are to set to `false`, that is, all supported fields will
/// be set to null pointers.
pub const USE_NONE: ToUse = ToUse {
    set_type: false,
    set_wake: false,
};

/// Defines the [`Chip::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_irq_chip_operations {
    () => {
        const TO_USE: $crate::irq::ToUse = $crate::irq::USE_NONE;
    };
    ($($i:ident),+) => {
        #[allow(clippy::needless_update)]
        const TO_USE: $crate::irq::ToUse =
            $crate::irq::ToUse {
                $($i: true),+ ,
                ..$crate::irq::USE_NONE
            };
    };
}

/// Enables or disables power-management wake-on for the given irq number.
pub fn set_wake(irq: u32, on: bool) -> Result {
    // SAFETY: Just an FFI call, there are no extra requirements for safety.
    let ret = unsafe { bindings::irq_set_irq_wake(irq, on as _) };
    if ret < 0 {
        Err(Error::from_kernel_errno(ret))
    } else {
        Ok(())
    }
}

unsafe extern "C" fn irq_ack_callback<T: Chip>(irq_data: *mut bindings::irq_data) {
    // SAFETY: The safety requirements of `init_chip`, which is the only place that uses this
    // callback, ensure that the value stored as irq chip data comes from a previous call to
    // `PointerWrapper::into_pointer`.
    let data = unsafe { T::Data::borrow(bindings::irq_data_get_irq_chip_data(irq_data)) };

    // SAFETY: The value returned by `IrqData` is only valid until the end of this function, and
    // `irq_data` is guaranteed to be valid until then (by the contract with C code).
    T::ack(data, unsafe { &IrqData::from_ptr(irq_data) })
}

unsafe extern "C" fn irq_mask_callback<T: Chip>(irq_data: *mut bindings::irq_data) {
    // SAFETY: The safety requirements of `init_chip`, which is the only place that uses this
    // callback, ensure that the value stored as irq chip data comes from a previous call to
    // `PointerWrapper::into_pointer`.
    let data = unsafe { T::Data::borrow(bindings::irq_data_get_irq_chip_data(irq_data)) };

    // SAFETY: The value returned by `IrqData` is only valid until the end of this function, and
    // `irq_data` is guaranteed to be valid until then (by the contract with C code).
    T::mask(data, unsafe { &IrqData::from_ptr(irq_data) })
}

unsafe extern "C" fn irq_unmask_callback<T: Chip>(irq_data: *mut bindings::irq_data) {
    // SAFETY: The safety requirements of `init_chip`, which is the only place that uses this
    // callback, ensure that the value stored as irq chip data comes from a previous call to
    // `PointerWrapper::into_pointer`.
    let data = unsafe { T::Data::borrow(bindings::irq_data_get_irq_chip_data(irq_data)) };

    // SAFETY: The value returned by `IrqData` is only valid until the end of this function, and
    // `irq_data` is guaranteed to be valid until then (by the contract with C code).
    T::unmask(data, unsafe { &IrqData::from_ptr(irq_data) })
}

unsafe extern "C" fn irq_set_type_callback<T: Chip>(
    irq_data: *mut bindings::irq_data,
    flow_type: core::ffi::c_uint,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The safety requirements of `init_chip`, which is the only place that uses this
        // callback, ensure that the value stored as irq chip data comes from a previous call to
        // `PointerWrapper::into_pointer`.
        let data = unsafe { T::Data::borrow(bindings::irq_data_get_irq_chip_data(irq_data)) };

        // SAFETY: The value returned by `IrqData` is only valid until the end of this function, and
        // `irq_data` is guaranteed to be valid until then (by the contract with C code).
        let ret = T::set_type(data, &mut LockedIrqData(unsafe { IrqData::from_ptr(irq_data) }), flow_type)?;
        Ok(ret as _)
    }
}

unsafe extern "C" fn irq_set_wake_callback<T: Chip>(
    irq_data: *mut bindings::irq_data,
    on: core::ffi::c_uint,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The safety requirements of `init_chip`, which is the only place that uses this
        // callback, ensure that the value stored as irq chip data comes from a previous call to
        // `PointerWrapper::into_pointer`.
        let data = unsafe { T::Data::borrow(bindings::irq_data_get_irq_chip_data(irq_data)) };

        // SAFETY: The value returned by `IrqData` is only valid until the end of this function, and
        // `irq_data` is guaranteed to be valid until then (by the contract with C code).
        T::set_wake(data, unsafe { &IrqData::from_ptr(irq_data) }, on != 0)?;
        Ok(0)
    }
}

/// Contains constants that describes how an interrupt can be triggered.
///
/// It is tagged with `non_exhaustive` to prevent users from instantiating it.
#[non_exhaustive]
pub struct Type;

impl Type {
    /// The interrupt cannot be triggered.
    pub const NONE: u32 = bindings::IRQ_TYPE_NONE;

    /// The interrupt is triggered when the signal goes from low to high.
    pub const EDGE_RISING: u32 = bindings::IRQ_TYPE_EDGE_RISING;

    /// The interrupt is triggered when the signal goes from high to low.
    pub const EDGE_FALLING: u32 = bindings::IRQ_TYPE_EDGE_FALLING;

    /// The interrupt is triggered when the signal goes from low to high and when it goes to high
    /// to low.
    pub const EDGE_BOTH: u32 = bindings::IRQ_TYPE_EDGE_BOTH;

    /// The interrupt is triggered while the signal is held high.
    pub const LEVEL_HIGH: u32 = bindings::IRQ_TYPE_LEVEL_HIGH;

    /// The interrupt is triggered while the signal is held low.
    pub const LEVEL_LOW: u32 = bindings::IRQ_TYPE_LEVEL_LOW;
}

/// Wraps the kernel's `struct irq_desc`.
///
/// # Invariants
///
/// The pointer `Descriptor::ptr` is non-null and valid.
pub struct Descriptor {
    pub(crate) ptr: *mut bindings::irq_desc,
}

impl Descriptor {
    /// Constructs a new `struct irq_desc` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    unsafe fn from_ptr(ptr: *mut bindings::irq_desc) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }

    /// Calls `chained_irq_enter` and returns a guard that calls `chained_irq_exit` once dropped.
    ///
    /// It is meant to be used by chained irq handlers to dispatch irqs to the next handlers.
    pub fn enter_chained(&self) -> ChainedGuard<'_> {
        // SAFETY: By the type invariants, `ptr` is always non-null and valid.
        let irq_chip = unsafe { bindings::irq_desc_get_chip(self.ptr) };

        // SAFETY: By the type invariants, `ptr` is always non-null and valid. `irq_chip` was just
        // returned from `ptr`, so it is still valid too.
        unsafe { bindings::chained_irq_enter(irq_chip, self.ptr) };
        ChainedGuard {
            desc: self,
            irq_chip,
        }
    }
}

struct InternalRegistration<T: PointerWrapper> {
    irq: u32,
    data: *mut core::ffi::c_void,
    name: CString,
    _p: PhantomData<T>,
}

impl<T: PointerWrapper> InternalRegistration<T> {
    /// Registers a new irq handler.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `handler` and `thread_fn` are compatible with the registration,
    /// that is, that they only use their second argument while the call is happening and that they
    /// only call [`T::borrow`] on it (e.g., they shouldn't call [`T::from_pointer`] and consume
    /// it).
    unsafe fn try_new(
        irq: core::ffi::c_uint,
        handler: bindings::irq_handler_t,
        thread_fn: bindings::irq_handler_t,
        flags: usize,
        data: T,
        name: fmt::Arguments<'_>,
    ) -> Result<Self> {
        let ptr = data.into_pointer() as *mut _;
        let name = CString::try_from_fmt(name)?;
        let guard = ScopeGuard::new(|| {
            // SAFETY: `ptr` came from a previous call to `into_pointer`.
            unsafe { T::from_pointer(ptr) };
        });
        // SAFETY: `name` and `ptr` remain valid as long as the registration is alive.
        to_result(unsafe {
            bindings::request_threaded_irq(
                irq,
                handler,
                thread_fn,
                flags as _,
                name.as_char_ptr(),
                ptr,
            )
        })?;
        guard.dismiss();
        Ok(Self {
            irq,
            name,
            data: ptr,
            _p: PhantomData,
        })
    }
}

impl<T: PointerWrapper> Drop for InternalRegistration<T> {
    fn drop(&mut self) {
        // Unregister irq handler.
        //
        // SAFETY: When `try_new` succeeds, the irq was successfully requested, so it is ok to free
        // it here.
        unsafe { bindings::free_irq(self.irq, self.data) };

        // Free context data.
        //
        // SAFETY: This matches the call to `into_pointer` from `try_new` in the success case.
        unsafe { T::from_pointer(self.data) };
    }
}

/// An irq handler.
pub trait Handler {
    /// The context data associated with and made available to the handler.
    type Data: PointerWrapper;

    /// Called from interrupt context when the irq happens.
    fn handle_irq(data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Return;
}

/// The registration of an interrupt handler.
///
/// # Examples
///
/// The following is an example of a regular handler with a boxed `u32` as data.
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::irq;
///
/// struct Example;
///
/// impl irq::Handler for Example {
///     type Data = Box<u32>;
///
///     fn handle_irq(_data: &u32) -> irq::Return {
///         irq::Return::None
///     }
/// }
///
/// fn request_irq(irq: u32, data: Box<u32>) -> Result<irq::Registration<Example>> {
///     irq::Registration::try_new(
///         irq, data, irq::flags::SHARED, fmt!("example_{irq}"))
/// }
/// ```
pub struct Registration<H: Handler>(InternalRegistration<H::Data>);

impl<H: Handler> Registration<H> {
    /// Registers a new irq handler.
    ///
    /// The valid values of `flags` come from the [`flags`] module.
    pub fn try_new(
        irq: u32,
        data: H::Data,
        flags: usize,
        name: fmt::Arguments<'_>,
    ) -> Result<Self> {
        // SAFETY: `handler` only calls `H::Data::borrow` on `raw_data`.
        Ok(Self(unsafe {
            InternalRegistration::try_new(irq, Some(Self::handler), None, flags, data, name)?
        }))
    }

    unsafe extern "C" fn handler(
        _irq: core::ffi::c_int,
        raw_data: *mut core::ffi::c_void,
    ) -> bindings::irqreturn_t {
        // SAFETY: On registration, `into_pointer` was called, so it is safe to borrow from it here
        // because `from_pointer` is called only after the irq is unregistered.
        let data = unsafe { H::Data::borrow(raw_data) };
        H::handle_irq(data) as _
    }
}

/// A threaded irq handler.
pub trait ThreadedHandler {
    /// The context data associated with and made available to the handlers.
    type Data: PointerWrapper;

    /// Called from interrupt context when the irq first happens.
    fn handle_primary_irq(_data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Return {
        Return::WakeThread
    }

    /// Called from the handler thread.
    fn handle_threaded_irq(data: <Self::Data as PointerWrapper>::Borrowed<'_>) -> Return;
}

/// The registration of a threaded interrupt handler.
///
/// # Examples
///
/// The following is an example of a threaded handler with a ref-counted u32 as data:
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::{irq, sync::{Ref, RefBorrow}};
///
/// struct Example;
///
/// impl irq::ThreadedHandler for Example {
///     type Data = Ref<u32>;
///
///     fn handle_threaded_irq(_data: RefBorrow<'_, u32>) -> irq::Return {
///         irq::Return::None
///     }
/// }
///
/// fn request_irq(irq: u32, data: Ref<u32>) -> Result<irq::ThreadedRegistration<Example>> {
///     irq::ThreadedRegistration::try_new(
///         irq, data, irq::flags::SHARED, fmt!("example_{irq}"))
/// }
/// ```
pub struct ThreadedRegistration<H: ThreadedHandler>(InternalRegistration<H::Data>);

impl<H: ThreadedHandler> ThreadedRegistration<H> {
    /// Registers a new threaded irq handler.
    ///
    /// The valid values of `flags` come from the [`flags`] module.
    pub fn try_new(
        irq: u32,
        data: H::Data,
        flags: usize,
        name: fmt::Arguments<'_>,
    ) -> Result<Self> {
        // SAFETY: both `primary_handler` and `threaded_handler` only call `H::Data::borrow` on
        // `raw_data`.
        Ok(Self(unsafe {
            InternalRegistration::try_new(
                irq,
                Some(Self::primary_handler),
                Some(Self::threaded_handler),
                flags,
                data,
                name,
            )?
        }))
    }

    unsafe extern "C" fn primary_handler(
        _irq: core::ffi::c_int,
        raw_data: *mut core::ffi::c_void,
    ) -> bindings::irqreturn_t {
        // SAFETY: On registration, `into_pointer` was called, so it is safe to borrow from it here
        // because `from_pointer` is called only after the irq is unregistered.
        let data = unsafe { H::Data::borrow(raw_data) };
        H::handle_primary_irq(data) as _
    }

    unsafe extern "C" fn threaded_handler(
        _irq: core::ffi::c_int,
        raw_data: *mut core::ffi::c_void,
    ) -> bindings::irqreturn_t {
        // SAFETY: On registration, `into_pointer` was called, so it is safe to borrow from it here
        // because `from_pointer` is called only after the irq is unregistered.
        let data = unsafe { H::Data::borrow(raw_data) };
        H::handle_threaded_irq(data) as _
    }
}

/// The return value from interrupt handlers.
pub enum Return {
    /// The interrupt was not from this device or was not handled.
    None = bindings::irqreturn_IRQ_NONE as _,

    /// The interrupt was handled by this device.
    Handled = bindings::irqreturn_IRQ_HANDLED as _,

    /// The handler wants the handler thread to wake up.
    WakeThread = bindings::irqreturn_IRQ_WAKE_THREAD as _,
}

/// Container for interrupt flags.
pub mod flags {
    use crate::bindings;

    /// Use the interrupt line as already configured.
    pub const TRIGGER_NONE: usize = bindings::IRQF_TRIGGER_NONE as _;

    /// The interrupt is triggered when the signal goes from low to high.
    pub const TRIGGER_RISING: usize = bindings::IRQF_TRIGGER_RISING as _;

    /// The interrupt is triggered when the signal goes from high to low.
    pub const TRIGGER_FALLING: usize = bindings::IRQF_TRIGGER_FALLING as _;

    /// The interrupt is triggered while the signal is held high.
    pub const TRIGGER_HIGH: usize = bindings::IRQF_TRIGGER_HIGH as _;

    /// The interrupt is triggered while the signal is held low.
    pub const TRIGGER_LOW: usize = bindings::IRQF_TRIGGER_LOW as _;

    /// Allow sharing the irq among several devices.
    pub const SHARED: usize = bindings::IRQF_SHARED as _;

    /// Set by callers when they expect sharing mismatches to occur.
    pub const PROBE_SHARED: usize = bindings::IRQF_PROBE_SHARED as _;

    /// Flag to mark this interrupt as timer interrupt.
    pub const TIMER: usize = bindings::IRQF_TIMER as _;

    /// Interrupt is per cpu.
    pub const PERCPU: usize = bindings::IRQF_PERCPU as _;

    /// Flag to exclude this interrupt from irq balancing.
    pub const NOBALANCING: usize = bindings::IRQF_NOBALANCING as _;

    /// Interrupt is used for polling (only the interrupt that is registered first in a shared
    /// interrupt is considered for performance reasons).
    pub const IRQPOLL: usize = bindings::IRQF_IRQPOLL as _;

    /// Interrupt is not reenabled after the hardirq handler finished. Used by threaded interrupts
    /// which need to keep the irq line disabled until the threaded handler has been run.
    pub const ONESHOT: usize = bindings::IRQF_ONESHOT as _;

    /// Do not disable this IRQ during suspend. Does not guarantee that this interrupt will wake
    /// the system from a suspended state.
    pub const NO_SUSPEND: usize = bindings::IRQF_NO_SUSPEND as _;

    /// Force enable it on resume even if [`NO_SUSPEND`] is set.
    pub const FORCE_RESUME: usize = bindings::IRQF_FORCE_RESUME as _;

    /// Interrupt cannot be threaded.
    pub const NO_THREAD: usize = bindings::IRQF_NO_THREAD as _;

    /// Resume IRQ early during syscore instead of at device resume time.
    pub const EARLY_RESUME: usize = bindings::IRQF_EARLY_RESUME as _;

    /// If the IRQ is shared with a NO_SUSPEND user, execute this interrupt handler after
    /// suspending interrupts. For system wakeup devices users need to implement wakeup detection
    /// in their interrupt handlers.
    pub const COND_SUSPEND: usize = bindings::IRQF_COND_SUSPEND as _;

    /// Don't enable IRQ or NMI automatically when users request it. Users will enable it
    /// explicitly by `enable_irq` or `enable_nmi` later.
    pub const NO_AUTOEN: usize = bindings::IRQF_NO_AUTOEN as _;

    /// Exclude from runnaway detection for IPI and similar handlers, depends on `PERCPU`.
    pub const NO_DEBUG: usize = bindings::IRQF_NO_DEBUG as _;
}

/// A guard to call `chained_irq_exit` after `chained_irq_enter` was called.
///
/// It is also used as evidence that a previous `chained_irq_enter` was called. So there are no
/// public constructors and it is only created after indeed calling `chained_irq_enter`.
pub struct ChainedGuard<'a> {
    desc: &'a Descriptor,
    irq_chip: *mut bindings::irq_chip,
}

impl Drop for ChainedGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: The lifetime of `ChainedGuard` guarantees that `self.desc` remains valid, so it
        // also guarantess `irq_chip` (which was returned from it) and `self.desc.ptr` (guaranteed
        // by the type invariants).
        unsafe { bindings::chained_irq_exit(self.irq_chip, self.desc.ptr) };
    }
}

/// Wraps the kernel's `struct irq_domain`.
///
/// # Invariants
///
/// The pointer `Domain::ptr` is non-null and valid.
#[cfg(CONFIG_IRQ_DOMAIN)]
pub struct Domain {
    ptr: *mut bindings::irq_domain,
}

#[cfg(CONFIG_IRQ_DOMAIN)]
impl Domain {
    /// Constructs a new `struct irq_domain` wrapper.
    ///
    /// # Safety
    ///
    /// The pointer `ptr` must be non-null and valid for the lifetime of the returned object.
    pub(crate) unsafe fn from_ptr(ptr: *mut bindings::irq_domain) -> Self {
        // INVARIANT: The safety requirements ensure the invariant.
        Self { ptr }
    }

    /// Invokes the chained handler of the given hw irq of the given domain.
    ///
    /// It requires evidence that `chained_irq_enter` was called, which is done by passing a
    /// `ChainedGuard` instance.
    pub fn generic_handle_chained(&self, hwirq: u32, _guard: &ChainedGuard<'_>) {
        // SAFETY: `ptr` is valid by the type invariants.
        unsafe { bindings::generic_handle_domain_irq(self.ptr, hwirq) };
    }
}

/// A high-level irq flow handler.
pub trait FlowHandler {
    /// The data associated with the handler.
    type Data: PointerWrapper;

    /// Implements the irq flow for the given descriptor.
    fn handle_irq_flow(data: <Self::Data as PointerWrapper>::Borrowed<'_>, desc: &Descriptor);
}

/// Returns the raw irq flow handler corresponding to the (high-level) one defined in `T`.
///
/// # Safety
///
/// The caller must ensure that the value stored in the irq handler data (as returned by
/// `irq_desc_get_handler_data`) is the result of calling [`PointerWrapper::into_pointer] for the
/// [`T::Data`] type.
pub(crate) unsafe fn new_flow_handler<T: FlowHandler>() -> bindings::irq_flow_handler_t {
    Some(irq_flow_handler::<T>)
}

unsafe extern "C" fn irq_flow_handler<T: FlowHandler>(desc: *mut bindings::irq_desc) {
    // SAFETY: By the safety requirements of `new_flow_handler`, we know that the value returned by
    // `irq_desc_get_handler_data` comes from calling `T::Data::into_pointer`. `desc` is valid by
    // the C API contract.
    let data = unsafe { T::Data::borrow(bindings::irq_desc_get_handler_data(desc)) };

    // SAFETY: The C API guarantees that `desc` is valid for the duration of this call, which
    // outlives the lifetime returned by `from_desc`.
    T::handle_irq_flow(data, &unsafe { Descriptor::from_ptr(desc) });
}
