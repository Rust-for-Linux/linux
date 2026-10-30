// SPDX-License-Identifier: GPL-2.0

//! Serial-core support for memory-mapped, two-wire polling consoles.
//!
//! C header: [`include/linux/serial_core.h`](srctree/include/linux/serial_core.h).
//!
//! [`Driver`] owns the serial driver and its legacy console. Each device-managed
//! port owns its hardware data and retains the driver until removal completes.
//! Registration failures release unpublished data, or remove a published port if
//! devres registration fails. Removal drains serial-core users and console SRCU
//! readers before releasing hardware data and the driver reference.
//!
//! The adapters acquire the UART lock for [`SerialOps::set_termios`] and normal
//! console writes. Oops output may proceed without the lock after a failed
//! trylock; it never receives a [`LockedPort`]. Callbacks must not sleep.
//! TTY registration stays in C serial core; TTY startup returns `EOPNOTSUPP`.
//! IRQ-driven I/O, reception and DMA are outside this polling interface.

use crate::{
    bindings,
    device::{Bound, Device},
    devres,
    error::to_result,
    prelude::*,
    sync::{aref::ARef, Arc},
    types::Opaque,
    ThisModule,
};
use core::marker::PhantomData;

mod console;
mod termios;

use self::console::ConsoleAdapter;
pub use self::termios::Termios;

/// A registered serial driver. Ports retain a reference until removal completes.
#[pin_data(PinnedDrop)]
pub struct Driver<T: SerialOps> {
    _type: PhantomData<T>,
    #[pin]
    console: Opaque<bindings::console>,
    #[pin]
    inner: Opaque<bindings::uart_driver>,
}

// SAFETY: The C objects are pinned. uart_driver state is allocated before publication;
// port_mutex serializes port changes. Each port retains an Arc until removal finishes.
unsafe impl<T: SerialOps> Send for Driver<T> {}
// SAFETY: C fields have interior mutability through Opaque. Console-list updates use
// console_list_lock and console_srcu; active console callbacks finish before removal.
// Driver names and callback pointers are immutable after initialization.
unsafe impl<T: SerialOps> Sync for Driver<T> {}

impl<T: SerialOps> Driver<T> {
    /// Registers a two-wire polling console with dynamic TTY device numbers.
    ///
    /// `baud` supplies the default when console options omit a baud rate.
    pub fn new(
        name: &'static CStr,
        device_name: &'static CStr,
        ports: u32,
        baud: u32,
        module: &'static ThisModule,
    ) -> impl PinInit<Self, Error> {
        try_pin_init!(&this in Self {
            _type: PhantomData,
            console <- Opaque::ffi_init(|ptr: *mut bindings::console| {
                // SAFETY: The console is private and pinned before driver publication.
                unsafe { ptr.write(bindings::console::default()) };
            }),
            inner <- Opaque::try_ffi_init(move |ptr: *mut bindings::uart_driver| {
                if ports == 0 || ports > i16::MAX as u32
                    || baud == 0 || baud > i32::MAX as u32
                    || device_name.to_bytes().len() >= 16 {
                    return Err(EINVAL);
                }
                // SAFETY: The initializer exclusively owns the pinned slot. The strings and
                // module outlive registration; private fields are initially zero.
                unsafe {
                    ptr.write(bindings::uart_driver::default());
                    (*ptr).owner = module.as_ptr();
                    (*ptr).driver_name = name.as_char_ptr();
                    (*ptr).dev_name = device_name.as_char_ptr();
                    (*ptr).nr = ports as i32;
                    let console = (*this.as_ptr()).console.get();
                    core::ptr::copy_nonoverlapping(
                        device_name.as_char_ptr(), (*console).name.as_mut_ptr(),
                        device_name.to_bytes().len(),
                    );
                    (*console).write = Some(ConsoleAdapter::<T>::console_write);
                    (*console).setup = Some(ConsoleAdapter::<T>::console_setup);
                    (*console).device = Some(bindings::uart_console_device);
                    (*console).flags = bindings::cons_flags_CON_PRINTBUFFER as i16;
                    (*console).index = -1;
                    (*console).ispeed = baud;
                    (*console).ospeed = baud;
                    (*console).data = ptr.cast();
                    (*ptr).cons = console;
                    to_result(bindings::uart_register_driver(ptr))
                }
            }),
        })
    }
}

#[pinned_drop]
impl<T: SerialOps> PinnedDrop for Driver<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: Registration succeeded and every port holds an Arc to this driver.
        unsafe { bindings::uart_unregister_driver(self.inner.get()) };
    }
}

/// Serial hardware callbacks currently needed by a two-wire polling console.
///
/// This is a subset of serial-core operations, not a full UART interface.
/// Reception, interrupt-driven transmission and DMA are not supported.
///
/// Callbacks must not sleep or allocate. The adapters lock the UART and save local
/// IRQ state for `set_termios` and normal `console_write` calls. During an oops,
/// console output may run without the lock. `tx_empty` acquires no lock and may
/// be called with the UART lock already held.
pub trait SerialOps: Send + Sync + Sized + 'static {
    /// Whether both the transmit FIFO and shift register are empty.
    fn tx_empty(&self, port: &Port<'_>) -> bool;
    /// Applies console framing and baud rate, reflecting unsupported modes in `new`.
    ///
    /// The UART lock is held and local IRQs are disabled throughout this call.
    fn set_termios(&self, port: &LockedPort<'_>, new: &mut Termios, old: Option<&Termios>);
    /// Writes a console record using bounded polling, including during an oops.
    fn console_write(&self, port: &Port<'_>, bytes: &[u8]);
}

/// Configuration supplied before registering a memory-mapped UART.
pub struct PortConfig {
    /// Serial-core line index.
    pub line: u32,
    /// Physical register base, for serial-core reporting.
    pub mapbase: bindings::resource_size_t,
    /// Baud generator frequency in Hz.
    pub clock: u32,
    /// Hardware FIFO capacity in bytes.
    pub fifo_size: u32,
}

/// Registers an owned port until the device is unbound.
///
/// All state is owned and `'static`; no callback can retain a borrow into probe's
/// stack. The registration is transferred to devres rather than returned to the
/// caller. Forgetting a caller-owned object cannot leave borrowed data registered.
/// On devres allocation failure, the registration is removed before returning.
pub fn register_port<T: SerialOps>(
    driver: Arc<Driver<T>>,
    device: &Device<Bound>,
    config: PortConfig,
    data: impl PinInit<T, Error>,
) -> Result {
    // SAFETY: Driver registration owns its state array throughout this Arc's lifetime.
    if config.line >= unsafe { (*driver.inner.get()).nr as u32 } {
        return Err(EINVAL);
    }
    let port = KBox::pin_init(
        PortData::new(driver, device.into(), config, data),
        GFP_KERNEL,
    )?;
    // SAFETY: All callback state is initialized and pinned. The device borrow keeps
    // it bound until devres takes ownership. C retains nothing on registration failure.
    unsafe {
        to_result(bindings::uart_add_one_port(
            port.driver.inner.get(),
            port.inner.get(),
        ))?;
    }
    devres::register(device, Registration { port }, GFP_KERNEL)
}

#[pin_data]
struct PortData<T: SerialOps> {
    #[pin]
    inner: Opaque<bindings::uart_port>,
    #[pin]
    data: T,
    driver: Arc<Driver<T>>,
    device: ARef<Device>,
}

// SAFETY: uart_port's mutable C fields live in Opaque and use serial-core locking.
// Data is Send + Sync; device/driver references keep their allocations alive.
unsafe impl<T: SerialOps> Send for PortData<T> {}
// SAFETY: Rust-owned fields are immutable after publication. C state changes use
// port_mutex, tty_port.mutex, the UART lock and console-list/SRCU synchronization.
unsafe impl<T: SerialOps> Sync for PortData<T> {}

impl<T: SerialOps> PortData<T> {
    fn new(
        driver: Arc<Driver<T>>,
        device: ARef<Device>,
        config: PortConfig,
        data: impl PinInit<T, Error>,
    ) -> impl PinInit<Self, Error> {
        try_pin_init!(&this in Self {
            data <- data,
            driver,
            device,
            inner <- Opaque::ffi_init(|ptr: *mut bindings::uart_port| {
                // SAFETY: The slot is private and exclusively owned until registration.
                unsafe {
                    ptr.write(bindings::uart_port::default());
                    (*ptr).dev = device.as_raw();
                    (*ptr).line = config.line;
                    (*ptr).mapbase = config.mapbase;
                    (*ptr).uartclk = config.clock;
                    (*ptr).fifosize = config.fifo_size;
                    (*ptr).iotype = bindings::uart_iotype_UPIO_MEM;
                    (*ptr).type_ = bindings::PORT_GENERIC as u32;
                    bindings::uart_port_set_boot_autoconf(ptr);
                    (*ptr).ops = &SerialAdapter::<T>::OPS;
                    (*ptr).private_data = this.as_ptr().cast();
                }
            }),
        })
    }

    /// # Safety
    /// `ptr` must be a PortData<T> port in a serial-core callback. Registration
    /// owns its allocation until uart_remove_one_port returns; removal waits for
    /// console_srcu readers and serial-core users before devres releases resources.
    unsafe fn with_port<R>(
        ptr: *mut bindings::uart_port,
        f: impl for<'a> FnOnce(&'a Self, &Port<'a>) -> R,
    ) -> R {
        // SAFETY: The private_data type is fixed by Driver<T>/register_port<T>.
        let state = unsafe { &*(*ptr).private_data.cast::<Self>() };
        // SAFETY: Port removal drains this callback before device unbind can finish.
        let device = unsafe { state.device.as_bound() };
        f(state, &Port { raw: ptr, device })
    }
}

/// Converts serial-core callbacks to the hardware operations and lock contexts.
struct SerialAdapter<T: SerialOps>(PhantomData<T>);

impl<T: SerialOps> SerialAdapter<T> {
    const OPS: bindings::uart_ops = bindings::uart_ops {
        tx_empty: Some(Self::tx_empty),
        set_mctrl: Some(Self::set_mctrl),
        get_mctrl: Some(Self::get_mctrl),
        start_tx: Some(Self::inactive),
        stop_tx: Some(Self::inactive),
        stop_rx: Some(Self::inactive),
        startup: Some(Self::startup),
        shutdown: Some(Self::inactive),
        set_termios: Some(Self::set_termios),
        type_: Some(Self::port_type),
        config_port: Some(Self::config_port),
        verify_port: Some(Self::verify_port),
        ..pin_init::zeroed()
    };

    /// # Safety
    /// Must be called by serial core with a live port under the uart_ops contract.
    unsafe extern "C" fn tx_empty(ptr: *mut bindings::uart_port) -> u32 {
        // SAFETY: Serial core retains the registered port throughout this callback.
        unsafe {
            PortData::<T>::with_port(ptr, |state, port| {
                if state.data.tx_empty(port) {
                    bindings::TIOCSER_TEMT
                } else {
                    0
                }
            })
        }
    }

    unsafe extern "C" fn set_mctrl(_ptr: *mut bindings::uart_port, _value: u32) {}

    unsafe extern "C" fn get_mctrl(_ptr: *mut bindings::uart_port) -> u32 {
        bindings::TIOCM_CAR | bindings::TIOCM_DSR | bindings::TIOCM_CTS
    }

    unsafe extern "C" fn inactive(_ptr: *mut bindings::uart_port) {}

    unsafe extern "C" fn startup(_ptr: *mut bindings::uart_port) -> i32 {
        EOPNOTSUPP.to_errno()
    }

    /// # Safety
    /// Must be called by serial core with a live port under the uart_ops contract.
    unsafe extern "C" fn set_termios(
        ptr: *mut bindings::uart_port,
        new: *mut bindings::ktermios,
        old: *const bindings::ktermios,
    ) {
        // SAFETY: Serial core supplies live termios pointers and holds the port
        // registration through this callback. No view or termios reference escapes.
        unsafe {
            PortData::<T>::with_port(ptr, |state, port| {
                port.with_lock(|locked| {
                    state.data.set_termios(
                        locked,
                        &mut *new.cast(),
                        old.cast::<Termios>().as_ref(),
                    );
                });
            });
        }
    }

    /// # Safety
    /// Must be called by serial core with a live port under the uart_ops contract.
    unsafe extern "C" fn port_type(ptr: *mut bindings::uart_port) -> *const crate::ffi::c_char {
        // SAFETY: The registered port owns an Arc to its driver; driver_name is
        // an immutable static C string. The pointer, not a Rust borrow, is returned.
        unsafe { PortData::<T>::with_port(ptr, |state, _| (*state.driver.inner.get()).driver_name) }
    }

    /// # Safety
    /// Must be called by serial core with a live port under the uart_ops contract.
    unsafe extern "C" fn config_port(ptr: *mut bindings::uart_port, flags: i32) {
        if flags & bindings::UART_CONFIG_TYPE as i32 != 0 {
            // SAFETY: Serial core serializes port configuration.
            unsafe { (*ptr).type_ = bindings::PORT_GENERIC as u32 };
        }
    }

    /// # Safety
    /// Must be called by serial core with a live port under the uart_ops contract.
    unsafe extern "C" fn verify_port(
        _ptr: *mut bindings::uart_port,
        serial: *mut bindings::serial_struct,
    ) -> i32 {
        // SAFETY: Serial core supplies a valid serial_struct for the callback.
        let kind = unsafe { (*serial).type_ };
        if kind == bindings::PORT_UNKNOWN as i32 || kind == bindings::PORT_GENERIC {
            0
        } else {
            EINVAL.to_errno()
        }
    }
}

struct Registration<T: SerialOps> {
    port: Pin<KBox<PortData<T>>>,
}

impl<T: SerialOps> Drop for Registration<T> {
    fn drop(&mut self) {
        // SAFETY: Only successful registration constructs this guard. Serial core
        // hangs up TTY users, unregisters the console, waits for console_srcu readers
        // and drains uart_state references before returning. The allocation, hardware
        // data and Driver Arc are released only after this synchronization.
        unsafe {
            bindings::uart_remove_one_port(self.port.driver.inner.get(), self.port.inner.get())
        };
    }
}

/// A callback-scoped view of a registered port.
///
/// Instances are only created by the FFI adapters. The registration and its
/// device-managed hardware resources remain live throughout the callback.
/// This view exposes the callback context independently of the hardware data type.
///
/// # Invariants
///
/// `raw` points to a registered port belonging to `device`. The FFI adapter keeps
/// the port and its device resources live for the entire borrow.
pub struct Port<'a> {
    raw: *mut bindings::uart_port,
    device: &'a Device<Bound>,
}

impl Port<'_> {
    /// The bound parent device for accessing device-managed hardware resources.
    pub fn device(&self) -> &Device<Bound> {
        self.device
    }

    /// Runs a closure with the serial-core UART lock held and local IRQs saved.
    fn with_lock<R>(&self, f: impl FnOnce(&LockedPort<'_>) -> R) -> R {
        let mut flags = 0;
        // SAFETY: The port is initialized and valid for this borrow.
        unsafe { bindings::uart_port_lock_irqsave(self.raw, &mut flags) };
        let guard = PortGuard {
            port: self.raw,
            flags,
            unlock: true,
        };
        let locked = LockedPort {
            raw: self.raw,
            device: self.device(),
            _lifetime: PhantomData,
        };
        let result = f(&locked);
        drop(guard);
        result
    }
}

struct PortGuard {
    port: *mut bindings::uart_port,
    flags: crate::ffi::c_ulong,
    unlock: bool,
}

impl Drop for PortGuard {
    fn drop(&mut self) {
        if self.unlock {
            // SAFETY: The port is live and this guard acquired its UART lock.
            unsafe { bindings::uart_port_unlock_irqrestore(self.port, self.flags) };
        }
    }
}

/// A UART borrowed while its serial-core lock is held. Cannot escape the borrow.
pub struct LockedPort<'a> {
    raw: *mut bindings::uart_port,
    device: &'a Device<Bound>,
    _lifetime: PhantomData<&'a mut ()>,
}

impl LockedPort<'_> {
    /// The bound parent device for accessing device-managed hardware resources.
    pub fn device(&self) -> &Device<Bound> {
        self.device
    }
}
