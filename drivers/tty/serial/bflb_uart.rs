// SPDX-License-Identifier: GPL-2.0-or-later

//! Bouffalo Lab UART3 polling console.

use core::num::NonZeroU32;

use kernel::{
    bindings,
    clk::Clk,
    device::{Bound, Core, Device},
    driver,
    io::{
        mem::{DevresExclusiveIoMem, ExclusiveIoMem},
        Io, IoBase,
    },
    of, platform,
    prelude::*,
    processor,
    serial::{self, LockedPort, Port, Termios},
    sync::Arc,
};

const UTX_CONFIG: usize = 0x00;
const URX_CONFIG: usize = 0x04;
const BIT_PRD: usize = 0x08;
const DATA_CONFIG: usize = 0x0c;
const SW_MODE: usize = 0x1c;
const INT_MASK: usize = 0x24;
const INT_EN: usize = 0x2c;
const STATUS: usize = 0x30;
const FIFO_CONFIG0: usize = 0x80;
const FIFO_CONFIG1: usize = 0x84;
const FIFO_WDATA: usize = 0x88;

kernel::sync::global_lock! {
    // SAFETY: Initialized once in module init, before registering the platform driver.
    unsafe(uninit) static SERIAL: Mutex<Option<Arc<serial::Driver<Hardware>>>> = None;
}

struct SerialOwner;

impl SerialOwner {
    fn new(module: &'static ThisModule) -> Result<Self> {
        // SAFETY: Module init runs once per load, before any access to SERIAL.
        unsafe { SERIAL.init() };
        *SERIAL.lock() = Some(Arc::pin_init(
            serial::Driver::new(c"bflb_uart", c"ttyBFLB", 1, 115200, module),
            GFP_KERNEL,
        )?);
        Ok(Self)
    }
}

impl Drop for SerialOwner {
    fn drop(&mut self) {
        SERIAL.lock().take();
    }
}

#[pin_data]
struct UartModule {
    #[pin]
    _platform: driver::Registration<platform::Adapter<Uart>>,
    _serial: SerialOwner,
}

impl kernel::InPlaceModule for UartModule {
    fn init(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            _serial: SerialOwner::new(module)?,
            _platform <- driver::Registration::new(c"bflb_uart", module),
        })
    }
}

module! {
    type: UartModule,
    name: "bflb_uart",
    authors: ["Kwanghoon Son"],
    description: "Bouffalo Lab polling serial console",
    license: "GPL",
}

struct Uart;

kernel::of_device_table!(OF_TABLE, (), [(of::DeviceId::new(c"bflb,uart"), ())]);

impl platform::Driver for Uart {
    type IdInfo = ();
    type Data<'bound> = ();
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe<'bound>(
        pdev: &'bound platform::Device<Core<'_>>,
        _info: Option<&'bound ()>,
    ) -> impl PinInit<Self::Data<'bound>, Error> + 'bound {
        pin_init::pin_init_scope(move || {
            let dev: &Device<Bound> = pdev.as_ref();
            let mapbase = pdev.resource_by_index(0).ok_or(ENODEV)?.start();
            let io = pdev
                .io_request_by_index(0)
                .ok_or(ENODEV)?
                .iomap_exclusive_sized::<0x90>()?
                .into_devres()?;
            let baud = EnabledClock::get(dev, c"baud")?;
            let clock = u32::try_from(baud.0.rate().as_hz())?;
            if clock == 0 {
                return Err(EINVAL);
            }
            let serial = SERIAL.lock().as_ref().ok_or(ENODEV)?.clone();
            serial::register_port(
                serial,
                dev,
                serial::PortConfig {
                    line: 0,
                    mapbase,
                    clock,
                    fifo_size: 32,
                },
                Ok(Hardware {
                    io,
                    clock,
                    _baud: baud,
                }),
            )?;
            Ok(Ok::<(), Error>(()))
        })
    }
}

struct EnabledClock(Clk);

impl EnabledClock {
    fn get(dev: &Device, name: &CStr) -> Result<Self> {
        let clock = Clk::get(dev, Some(name))?;
        clock.prepare_enable()?;
        Ok(Self(clock))
    }
}

impl Drop for EnabledClock {
    fn drop(&mut self) {
        self.0.disable_unprepare();
    }
}

struct Hardware {
    io: DevresExclusiveIoMem<0x90>,
    clock: u32,
    _baud: EnabledClock,
}

struct Registers<'a> {
    io: &'a ExclusiveIoMem<'a, 0x90>,
}

impl Registers<'_> {
    fn read<const REG: usize>(&self) -> u32 {
        self.io.as_view().read32(REG)
    }

    fn write<const REG: usize>(&self, value: u32) {
        self.io.as_view().write32(value, REG);
    }

    fn console_poll(&self, ready: impl Fn() -> bool) -> bool {
        for _ in 0..100_000 {
            if ready() {
                return true;
            }
            processor::cpu_relax();
        }
        false
    }

    fn console_putchar(&self, ch: u8) -> bool {
        if !self.console_poll(|| self.read::<FIFO_CONFIG1>() & 0x3f != 0) {
            return false;
        }
        self.io.as_view().write8(ch, FIFO_WDATA);
        true
    }

    fn tx_empty(&self) -> bool {
        self.read::<FIFO_CONFIG1>() & 0x3f == 32 && self.read::<STATUS>() & 1 == 0
    }

    fn console_write(&self, bytes: &[u8]) {
        let mask = self.read::<INT_MASK>();
        self.write::<INT_MASK>(u32::MAX);
        self.write::<UTX_CONFIG>(self.read::<UTX_CONFIG>() | 1);
        let mut complete = true;
        for &ch in bytes {
            if ch == b'\n' && !self.console_putchar(b'\r') {
                complete = false;
                break;
            }
            if !self.console_putchar(ch) {
                complete = false;
                break;
            }
        }
        if complete {
            self.console_poll(|| self.tx_empty());
        }
        self.write::<INT_MASK>(mask);
    }
}

impl serial::SerialOps for Hardware {
    fn tx_empty(&self, port: &Port<'_>) -> bool {
        let Ok(io) = self.io.access(port.device()) else {
            return false;
        };
        Registers { io }.tx_empty()
    }

    fn set_termios(&self, port: &LockedPort<'_>, new: &mut Termios, old: Option<&Termios>) {
        let Ok(io) = self.io.access(port.device()) else {
            return;
        };
        let registers = Registers { io };
        let cflag = new.cflag() & !(bindings::CMSPAR | bindings::CRTSCTS);
        new.set_cflag(cflag);
        let mut tx = (new.char_size() - 1) << 8;
        if cflag & bindings::PARENB != 0 {
            tx |= 1 << 4;
            if cflag & bindings::PARODD != 0 {
                tx |= 1 << 5;
            }
        }
        tx |= if cflag & bindings::CSTOPB != 0 {
            3 << 11
        } else {
            1 << 11
        };
        tx |= 1 | (1 << 2);
        let baud = port.baud_rate(
            new,
            old,
            self.clock.div_ceil(65536),
            self.clock.min(4000000),
        );
        let divisor = ((u64::from(self.clock) + u64::from(baud) / 2) / u64::from(baud))
            .clamp(1, 65536) as u32;
        registers.console_poll(|| registers.tx_empty());
        registers.write::<INT_MASK>(u32::MAX);
        registers.write::<INT_EN>(0);
        registers.write::<FIFO_CONFIG0>(0);
        registers.write::<URX_CONFIG>(0);
        registers.write::<DATA_CONFIG>(0);
        registers.write::<SW_MODE>(0);
        registers.write::<BIT_PRD>((divisor - 1) | ((divisor - 1) << 16));
        registers.write::<UTX_CONFIG>(tx);
        port.configure(new, NonZeroU32::new(self.clock / divisor).unwrap());
    }

    fn console_write(&self, port: &Port<'_>, bytes: &[u8]) {
        let Ok(io) = self.io.access(port.device()) else {
            return;
        };
        let registers = Registers { io };
        registers.console_write(bytes);
    }
}
