// SPDX-License-Identifier: GPL-2.0

//! Support for gpio device drivers.
//!
//! C header: [`include/linux/gpio/driver.h`](../../../../include/linux/gpio/driver.h)

use crate::{
    bindings, device, error::code::*, error::from_kernel_result, types::PointerWrapper, Error,
    Result,
};
use core::{
    cell::UnsafeCell,
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
};

#[cfg(CONFIG_GPIOLIB_IRQCHIP)]
pub use irqchip::{ChipWithIrqChip, RegistrationWithIrqChip};

/// The direction of a gpio line.
pub enum LineDirection {
    /// Direction is input.
    In = bindings::GPIO_LINE_DIRECTION_IN as _,

    /// Direction is output.
    Out = bindings::GPIO_LINE_DIRECTION_OUT as _,
}

/// A gpio chip.
pub trait Chip {
    /// Context data associated with the gpio chip.
    ///
    /// It determines the type of the context data passed to each of the methods of the trait.
    type Data: PointerWrapper + Sync + Send;

    /// The methods to use to populate [`struct gpio_chip`]. This is typically populated with
    /// [`declare_gpio_chip_operations`].
    const TO_USE: ToUse;

    /// Returns the direction of the given gpio line.
    fn get_direction(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _offset: u32,
    ) -> Result<LineDirection> {
        Err(ENOTSUPP)
    }

    /// Configures the direction as input of the given gpio line.
    fn direction_input(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _offset: u32,
    ) -> Result {
        Err(EIO)
    }

    /// Configures the direction as output of the given gpio line.
    ///
    /// The value that will be initially output is also specified.
    fn direction_output(
        _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
        _offset: u32,
        _value: bool,
    ) -> Result {
        Err(ENOTSUPP)
    }

    /// Returns the current value of the given gpio line.
    fn get(_data: <Self::Data as PointerWrapper>::Borrowed<'_>, _offset: u32) -> Result<bool> {
        Err(EIO)
    }

    /// Sets the value of the given gpio line.
    fn set(_data: <Self::Data as PointerWrapper>::Borrowed<'_>, _offset: u32, _value: bool) {}
}

/// Represents which fields of [`struct gpio_chip`] should be populated with pointers.
///
/// This is typically populated with the [`declare_gpio_chip_operations`] macro.
pub struct ToUse {
    /// The `get_direction` field of [`struct gpio_chip`].
    pub get_direction: bool,

    /// The `direction_input` field of [`struct gpio_chip`].
    pub direction_input: bool,

    /// The `direction_output` field of [`struct gpio_chip`].
    pub direction_output: bool,

    /// The `get` field of [`struct gpio_chip`].
    pub get: bool,

    /// The `set` field of [`struct gpio_chip`].
    pub set: bool,
}

/// A constant version where all values are set to `false`, that is, all supported fields will be
/// set to null pointers.
pub const USE_NONE: ToUse = ToUse {
    get_direction: false,
    direction_input: false,
    direction_output: false,
    get: false,
    set: false,
};

/// Defines the [`Chip::TO_USE`] field based on a list of fields to be populated.
#[macro_export]
macro_rules! declare_gpio_chip_operations {
    () => {
        const TO_USE: $crate::gpio::ToUse = $crate::gpio::USE_NONE;
    };
    ($($i:ident),+) => {
        #[allow(clippy::needless_update)]
        const TO_USE: $crate::gpio::ToUse =
            $crate::gpio::ToUse {
                $($i: true),+ ,
                ..$crate::gpio::USE_NONE
            };
    };
}

/// A registration of a gpio chip.
pub struct Registration<T: Chip> {
    gc: UnsafeCell<bindings::gpio_chip>,
    parent: Option<device::Device>,
    _p: PhantomData<T>,
    _pin: PhantomPinned,
}

impl<T: Chip> Registration<T> {
    /// Creates a new [`Registration`] but does not register it yet.
    ///
    /// It is allowed to move.
    pub fn new() -> Self {
        Self {
            parent: None,
            gc: UnsafeCell::new(bindings::gpio_chip::default()),
            _pin: PhantomPinned,
            _p: PhantomData,
        }
    }

    /// Registers a gpio chip with the rest of the kernel.
    pub fn register(
        self: Pin<&mut Self>,
        gpio_count: u16,
        base: Option<i32>,
        parent: &dyn device::RawDevice,
        data: T::Data,
    ) -> Result {
        if self.parent.is_some() {
            // Already registered.
            return Err(EINVAL);
        }

        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        {
            let gc = this.gc.get_mut();

            // Set up the callbacks.
            gc.request = Some(bindings::gpiochip_generic_request);
            gc.free = Some(bindings::gpiochip_generic_free);
            if T::TO_USE.get_direction {
                gc.get_direction = Some(get_direction_callback::<T>);
            }
            if T::TO_USE.direction_input {
                gc.direction_input = Some(direction_input_callback::<T>);
            }
            if T::TO_USE.direction_output {
                gc.direction_output = Some(direction_output_callback::<T>);
            }
            if T::TO_USE.get {
                gc.get = Some(get_callback::<T>);
            }
            if T::TO_USE.set {
                gc.set = Some(set_callback::<T>);
            }

            // When a base is not explicitly given, use -1 for one to be picked.
            if let Some(b) = base {
                gc.base = b;
            } else {
                gc.base = -1;
            }

            gc.ngpio = gpio_count;
            gc.parent = parent.raw_device();
            gc.label = parent.name().as_char_ptr();

            // TODO: Define `gc.owner` as well.
        }

        let data_pointer = <T::Data as PointerWrapper>::into_pointer(data);
        // SAFETY: `gc` was initilised above, so it is valid.
        let ret = unsafe {
            bindings::gpiochip_add_data_with_key(
                this.gc.get(),
                data_pointer as _,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        if ret < 0 {
            // SAFETY: `data_pointer` was returned by `into_pointer` above.
            unsafe { T::Data::from_pointer(data_pointer) };
            return Err(Error::from_kernel_errno(ret));
        }

        this.parent = Some(device::Device::from_dev(parent));
        Ok(())
    }
}

// SAFETY: `Registration` doesn't offer any methods or access to fields when shared between threads
// or CPUs, so it is safe to share it.
unsafe impl<T: Chip> Sync for Registration<T> {}

// SAFETY: Registration with and unregistration from the gpio subsystem can happen from any thread.
// Additionally, `T::Data` (which is dropped during unregistration) is `Send`, so it is ok to move
// `Registration` to different threads.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: Chip> Send for Registration<T> {}

impl<T: Chip> Default for Registration<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Chip> Drop for Registration<T> {
    /// Removes the registration from the kernel if it has completed successfully before.
    fn drop(&mut self) {
        if self.parent.is_some() {
            // Get a pointer to the data stored in chip before destroying it.
            // SAFETY: `gc` was during registration, which is guaranteed to have succeeded (because
            // `parent` is `Some(_)`, so it remains valid.
            let data_pointer = unsafe { bindings::gpiochip_get_data(self.gc.get()) };

            // SAFETY: By the same argument above, `gc` is still valid.
            unsafe { bindings::gpiochip_remove(self.gc.get()) };

            // Free data as well.
            // SAFETY: `data_pointer` was returned by `into_pointer` during registration.
            unsafe { <T::Data as PointerWrapper>::from_pointer(data_pointer) };
        }
    }
}

unsafe extern "C" fn get_direction_callback<T: Chip>(
    gc: *mut bindings::gpio_chip,
    offset: core::ffi::c_uint,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };
        Ok(T::get_direction(data, offset)? as i32)
    }
}

unsafe extern "C" fn direction_input_callback<T: Chip>(
    gc: *mut bindings::gpio_chip,
    offset: core::ffi::c_uint,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };
        T::direction_input(data, offset)?;
        Ok(0)
    }
}

unsafe extern "C" fn direction_output_callback<T: Chip>(
    gc: *mut bindings::gpio_chip,
    offset: core::ffi::c_uint,
    value: core::ffi::c_int,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };
        T::direction_output(data, offset, value != 0)?;
        Ok(0)
    }
}

unsafe extern "C" fn get_callback<T: Chip>(
    gc: *mut bindings::gpio_chip,
    offset: core::ffi::c_uint,
) -> core::ffi::c_int {
    from_kernel_result! {
        // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
        let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };
        let v = T::get(data, offset)?;
        Ok(v as _)
    }
}

unsafe extern "C" fn set_callback<T: Chip>(
    gc: *mut bindings::gpio_chip,
    offset: core::ffi::c_uint,
    value: core::ffi::c_int,
) {
    // SAFETY: The value stored as chip data was returned by `into_pointer` during registration.
    let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };
    T::set(data, offset, value != 0);
}

#[cfg(CONFIG_GPIOLIB_IRQCHIP)]
mod irqchip {
    use super::*;
    use crate::irq;

    /// A gpio chip that includes an irq chip.
    pub trait ChipWithIrqChip: Chip {
        /// Implements the irq flow for the gpio chip.
        fn handle_irq_flow(
            _data: <Self::Data as PointerWrapper>::Borrowed<'_>,
            _desc: &irq::Descriptor,
            _domain: &irq::Domain,
        );
    }

    /// A registration of a gpio chip that includes an irq chip.
    pub struct RegistrationWithIrqChip<T: ChipWithIrqChip> {
        reg: Registration<T>,
        irq_chip: UnsafeCell<bindings::irq_chip>,
        parent_irq: u32,
    }

    impl<T: ChipWithIrqChip> RegistrationWithIrqChip<T> {
        /// Creates a new [`RegistrationWithIrqChip`] but does not register it yet.
        ///
        /// It is allowed to move.
        pub fn new() -> Self {
            Self {
                reg: Registration::new(),
                irq_chip: UnsafeCell::new(bindings::irq_chip::default()),
                parent_irq: 0,
            }
        }

        /// Registers a gpio chip and its irq chip with the rest of the kernel.
        pub fn register<U: irq::Chip<Data = T::Data>>(
            mut self: Pin<&mut Self>,
            gpio_count: u16,
            base: Option<i32>,
            parent: &dyn device::RawDevice,
            data: T::Data,
            parent_irq: u32,
        ) -> Result {
            if self.reg.parent.is_some() {
                // Already registered.
                return Err(EINVAL);
            }

            // SAFETY: We never move out of `this`.
            let this = unsafe { self.as_mut().get_unchecked_mut() };

            // Initialise the irq_chip.
            {
                let irq_chip = this.irq_chip.get_mut();
                irq_chip.name = parent.name().as_char_ptr();

                // SAFETY: The gpio subsystem configures a pointer to `gpio_chip` as the irq chip
                // data, so we use `IrqChipAdapter` to convert to the `T::Data`, which is the same
                // as `irq::Chip::Data` per the bound above.
                unsafe { irq::init_chip::<IrqChipAdapter<U>>(irq_chip) };
            }

            // Initialise gc irq state.
            {
                let girq = &mut this.reg.gc.get_mut().irq;
                girq.chip = this.irq_chip.get();
                // SAFETY: By leaving `parent_handler_data` set to `null`, the gpio subsystem
                // initialises it to a pointer to the gpio chip, which is what `FlowHandler<T>`
                // expects.
                girq.parent_handler = unsafe { irq::new_flow_handler::<FlowHandler<T>>() };
                girq.num_parents = 1;
                girq.parents = &mut this.parent_irq;
                this.parent_irq = parent_irq;
                girq.default_type = bindings::IRQ_TYPE_NONE;
                girq.handler = Some(bindings::handle_bad_irq);
            }

            // SAFETY: `reg` is pinned when `self` is.
            let pinned = unsafe { self.map_unchecked_mut(|r| &mut r.reg) };
            pinned.register(gpio_count, base, parent, data)
        }
    }

    impl<T: ChipWithIrqChip> Default for RegistrationWithIrqChip<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    // SAFETY: `RegistrationWithIrqChip` doesn't offer any methods or access to fields when shared
    // between threads or CPUs, so it is safe to share it.
    unsafe impl<T: ChipWithIrqChip> Sync for RegistrationWithIrqChip<T> {}

    // SAFETY: Registration with and unregistration from the gpio subsystem (including irq chips for
    // them) can happen from any thread. Additionally, `T::Data` (which is dropped during
    // unregistration) is `Send`, so it is ok to move `Registration` to different threads.
    #[allow(clippy::non_send_fields_in_send_ty)]
    unsafe impl<T: ChipWithIrqChip> Send for RegistrationWithIrqChip<T> where T::Data: Send {}

    struct FlowHandler<T: ChipWithIrqChip>(PhantomData<T>);

    impl<T: ChipWithIrqChip> irq::FlowHandler for FlowHandler<T> {
        type Data = *mut bindings::gpio_chip;

        fn handle_irq_flow(gc: *mut bindings::gpio_chip, desc: &irq::Descriptor) {
            // SAFETY: `FlowHandler` is only used in gpio chips, and it is removed when the gpio is
            // unregistered, so we know that `gc` must still be valid. We also know that the value
            // stored as gpio data was returned by `T::Data::into_pointer` again because
            // `FlowHandler` is a private structure only used in this way.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc)) };

            // SAFETY: `gc` is valid (see comment above), so we can dereference it.
            let domain = unsafe { irq::Domain::from_ptr((*gc).irq.domain) };

            T::handle_irq_flow(data, desc, &domain);
        }
    }

    /// Adapter from an irq chip with `gpio_chip` pointer as context to one where the gpio chip
    /// data is passed as context.
    struct IrqChipAdapter<T: irq::Chip>(PhantomData<T>);

    impl<T: irq::Chip> irq::Chip for IrqChipAdapter<T> {
        type Data = *mut bindings::gpio_chip;
        const TO_USE: irq::ToUse = T::TO_USE;

        fn ack(gc: *mut bindings::gpio_chip, irq_data: &irq::IrqData) {
            // SAFETY: `IrqChipAdapter` is a private struct, only used when the data stored in the
            // gpio chip is known to come from `T::Data`, and only valid while the gpio chip is
            // registered, so `gc` is valid.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc as _)) };
            T::ack(data, irq_data);
        }

        fn mask(gc: *mut bindings::gpio_chip, irq_data: &irq::IrqData) {
            // SAFETY: `IrqChipAdapter` is a private struct, only used when the data stored in the
            // gpio chip is known to come from `T::Data`, and only valid while the gpio chip is
            // registered, so `gc` is valid.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc as _)) };
            T::mask(data, irq_data);
        }

        fn unmask(gc: *mut bindings::gpio_chip, irq_data: &irq::IrqData) {
            // SAFETY: `IrqChipAdapter` is a private struct, only used when the data stored in the
            // gpio chip is known to come from `T::Data`, and only valid while the gpio chip is
            // registered, so `gc` is valid.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc as _)) };
            T::unmask(data, irq_data);
        }

        fn set_type(
            gc: *mut bindings::gpio_chip,
            irq_data: &mut irq::LockedIrqData,
            flow_type: u32,
        ) -> Result<irq::ExtraResult> {
            // SAFETY: `IrqChipAdapter` is a private struct, only used when the data stored in the
            // gpio chip is known to come from `T::Data`, and only valid while the gpio chip is
            // registered, so `gc` is valid.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc as _)) };
            T::set_type(data, irq_data, flow_type)
        }

        fn set_wake(gc: *mut bindings::gpio_chip, irq_data: &irq::IrqData, on: bool) -> Result {
            // SAFETY: `IrqChipAdapter` is a private struct, only used when the data stored in the
            // gpio chip is known to come from `T::Data`, and only valid while the gpio chip is
            // registered, so `gc` is valid.
            let data = unsafe { T::Data::borrow(bindings::gpiochip_get_data(gc as _)) };
            T::set_wake(data, irq_data, on)
        }
    }
}
