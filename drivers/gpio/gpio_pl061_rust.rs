// SPDX-License-Identifier: GPL-2.0

//! Driver for the ARM PrimeCell(tm) General Purpose Input/Output (PL061).
//!
//! Based on the C driver written by Baruch Siach <baruch@tkos.co.il>.

use kernel::{
    amba, bit, bits_iter, define_amba_id_table, device, gpio,
    io_mem::IoMem,
    irq::{self, ExtraResult, IrqData, LockedIrqData},
    power,
    prelude::*,
    sync::{Arc, ArcBorrow, RawSpinLock},
};

const GPIODIR: usize = 0x400;
const GPIOIS: usize = 0x404;
const GPIOIBE: usize = 0x408;
const GPIOIEV: usize = 0x40C;
const GPIOIE: usize = 0x410;
const GPIOMIS: usize = 0x418;
const GPIOIC: usize = 0x41C;
const GPIO_SIZE: usize = 0x1000;

const PL061_GPIO_NR: u16 = 8;

#[derive(Default)]
struct ContextSaveRegs {
    gpio_data: u8,
    gpio_dir: u8,
    gpio_is: u8,
    gpio_ibe: u8,
    gpio_iev: u8,
    gpio_ie: u8,
}

#[derive(Default)]
struct PL061DataInner {
    csave_regs: ContextSaveRegs,
}

struct PL061Data {
    dev: device::Device,
    inner: RawSpinLock<PL061DataInner>,
}

struct PL061Resources {
    base: IoMem<GPIO_SIZE>,
    parent_irq: u32,
}

type PL061Registrations = gpio::RegistrationWithIrqChip<PL061Device>;

type DeviceData = device::Data<PL061Registrations, PL061Resources, PL061Data>;

struct PL061Device;

#[vtable]
impl gpio::Chip for PL061Device {
    type Data = Arc<DeviceData>;

    fn get_direction(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result<gpio::LineDirection> {
        let pl061 = data.resources().ok_or(ENXIO)?;
        Ok(if pl061.base.readb(GPIODIR) & bit(offset) != 0 {
            gpio::LineDirection::Out
        } else {
            gpio::LineDirection::In
        })
    }

    fn direction_input(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result {
        let _guard = data.inner.lock_irqdisable();
        let pl061 = data.resources().ok_or(ENXIO)?;
        let mut gpiodir = pl061.base.readb(GPIODIR);
        gpiodir &= !bit(offset);
        pl061.base.writeb(gpiodir, GPIODIR);
        Ok(())
    }

    fn direction_output(data: ArcBorrow<'_, DeviceData>, offset: u32, value: bool) -> Result {
        let woffset = bit(offset + 2).into();
        let _guard = data.inner.lock_irqdisable();
        let pl061 = data.resources().ok_or(ENXIO)?;
        pl061.base.try_writeb((value as u8) << offset, woffset)?;
        let mut gpiodir = pl061.base.readb(GPIODIR);
        gpiodir |= bit(offset);
        pl061.base.writeb(gpiodir, GPIODIR);

        // gpio value is set again, because pl061 doesn't allow to set value of a gpio pin before
        // configuring it in OUT mode.
        pl061.base.try_writeb((value as u8) << offset, woffset)?;
        Ok(())
    }

    fn get(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result<bool> {
        let pl061 = data.resources().ok_or(ENXIO)?;
        Ok(pl061.base.try_readb(bit(offset + 2).into())? != 0)
    }

    fn set(data: ArcBorrow<'_, DeviceData>, offset: u32, value: bool) {
        if let Some(pl061) = data.resources() {
            let woffset = bit(offset + 2).into();
            let _ = pl061.base.try_writeb((value as u8) << offset, woffset);
        }
    }
}

impl gpio::ChipWithIrqChip for PL061Device {
    fn handle_irq_flow(
        data: ArcBorrow<'_, DeviceData>,
        desc: &irq::Descriptor,
        domain: &irq::Domain,
    ) {
        let chained = desc.enter_chained();

        if let Some(pl061) = data.resources() {
            let pending = pl061.base.readb(GPIOMIS);
            for offset in bits_iter(pending) {
                domain.generic_handle_chained(offset, &chained);
            }
        }
    }
}

#[vtable]
impl irq::Chip for PL061Device {
    type Data = Arc<DeviceData>;

    fn set_type(
        data: ArcBorrow<'_, DeviceData>,
        irq_data: &mut LockedIrqData,
        trigger: u32,
    ) -> Result<ExtraResult> {
        let offset = irq_data.hwirq();
        let bit = bit(offset);

        if offset >= PL061_GPIO_NR.into() {
            return Err(EINVAL);
        }

        if trigger & (irq::Type::LEVEL_HIGH | irq::Type::LEVEL_LOW) != 0
            && trigger & (irq::Type::EDGE_RISING | irq::Type::EDGE_FALLING) != 0
        {
            dev_err!(
                data.dev,
                "trying to configure line {offset} for both level and edge detection, choose one!\n",
            );
            return Err(EINVAL);
        }

        let _guard = data.inner.lock_irqdisable();
        let pl061 = data.resources().ok_or(ENXIO)?;

        let mut gpioiev = pl061.base.readb(GPIOIEV);
        let mut gpiois = pl061.base.readb(GPIOIS);
        let mut gpioibe = pl061.base.readb(GPIOIBE);

        if trigger & (irq::Type::LEVEL_HIGH | irq::Type::LEVEL_LOW) != 0 {
            let polarity = trigger & irq::Type::LEVEL_HIGH != 0;

            // Disable edge detection.
            gpioibe &= !bit;
            // Enable level detection.
            gpiois |= bit;
            // Select polarity.
            if polarity {
                gpioiev |= bit;
            } else {
                gpioiev &= !bit;
            }
            irq_data.set_level_handler();
            dev_dbg!(
                data.dev,
                "line {offset}: IRQ on {} level\n",
                if polarity { "HIGH" } else { "LOW" }
            );
        } else if (trigger & irq::Type::EDGE_BOTH) == irq::Type::EDGE_BOTH {
            // Disable level detection.
            gpiois &= !bit;
            // Select both edges, settings this makes GPIOEV be ignored.
            gpioibe |= bit;
            irq_data.set_edge_handler();
            dev_dbg!(data.dev, "line {offset}: IRQ on both edges\n");
        } else if trigger & (irq::Type::EDGE_RISING | irq::Type::EDGE_FALLING) != 0 {
            let rising = trigger & irq::Type::EDGE_RISING != 0;

            // Disable level detection.
            gpiois &= !bit;
            // Clear detection on both edges.
            gpioibe &= !bit;
            // Select edge.
            if rising {
                gpioiev |= bit;
            } else {
                gpioiev &= !bit;
            }
            irq_data.set_edge_handler();
            dev_dbg!(
                data.dev,
                "line {offset}: IRQ on {} edge\n",
                if rising { "RISING" } else { "FALLING}" }
            );
        } else {
            // No trigger: disable everything.
            gpiois &= !bit;
            gpioibe &= !bit;
            gpioiev &= !bit;
            irq_data.set_bad_handler();
            dev_warn!(data.dev, "no trigger selected for line {offset}\n");
        }

        pl061.base.writeb(gpiois, GPIOIS);
        pl061.base.writeb(gpioibe, GPIOIBE);
        pl061.base.writeb(gpioiev, GPIOIEV);

        Ok(ExtraResult::None)
    }

    fn mask(data: ArcBorrow<'_, DeviceData>, irq_data: &IrqData) {
        let mask = bit(irq_data.hwirq() % irq::HwNumber::from(PL061_GPIO_NR));
        let _guard = data.inner.lock();
        if let Some(pl061) = data.resources() {
            let gpioie = pl061.base.readb(GPIOIE) & !mask;
            pl061.base.writeb(gpioie, GPIOIE);
        }
    }

    fn unmask(data: ArcBorrow<'_, DeviceData>, irq_data: &IrqData) {
        let mask = bit(irq_data.hwirq() % irq::HwNumber::from(PL061_GPIO_NR));
        let _guard = data.inner.lock();
        if let Some(pl061) = data.resources() {
            let gpioie = pl061.base.readb(GPIOIE) | mask;
            pl061.base.writeb(gpioie, GPIOIE);
        }
    }

    // This gets called from the edge IRQ handler to ACK the edge IRQ in the GPIOIC
    // (interrupt-clear) register. For level IRQs this is not needed: these go away when the level
    // signal goes away.
    fn ack(data: ArcBorrow<'_, DeviceData>, irq_data: &IrqData) {
        let mask = bit(irq_data.hwirq() % irq::HwNumber::from(PL061_GPIO_NR));
        let _guard = data.inner.lock();
        if let Some(pl061) = data.resources() {
            pl061.base.writeb(mask.into(), GPIOIC);
        }
    }

    fn set_wake(data: ArcBorrow<'_, DeviceData>, _irq_data: &IrqData, on: bool) -> Result {
        let pl061 = data.resources().ok_or(ENXIO)?;
        irq::set_wake(pl061.parent_irq, on)
    }
}

impl amba::Driver for PL061Device {
    type Data = Arc<DeviceData>;
    type PowerOps = Self;

    define_amba_id_table! {(), [
        ({id: 0x00041061, mask: 0x000fffff}, None),
    ]}

    fn probe(dev: &mut amba::Device, _data: Option<&Self::IdInfo>) -> Result<Arc<DeviceData>> {
        let res = dev.take_resource().ok_or(ENXIO)?;
        let irq = dev.irq(0).ok_or(ENXIO)?;

        let mut data = kernel::new_device_data!(
            gpio::RegistrationWithIrqChip::new(),
            PL061Resources {
                // SAFETY: This device doesn't support DMA.
                base: unsafe { IoMem::try_new(res)? },
                parent_irq: irq,
            },
            PL061Data {
                dev: device::Device::from_dev(dev),
                // SAFETY: We call `rawspinlock_init` below.
                inner: unsafe { RawSpinLock::new(PL061DataInner::default()) },
            },
            "PL061::Registrations"
        )?;

        // SAFETY: General part of the data is pinned when `data` is.
        let gen_inner = unsafe { data.as_mut().map_unchecked_mut(|d| &mut d.inner) };
        kernel::rawspinlock_init!(gen_inner, "PL061Data::inner");

        let data = Arc::<DeviceData>::from(data);

        data.resources().ok_or(ENXIO)?.base.writeb(0, GPIOIE); // disable irqs

        kernel::gpio_irq_chip_register!(
            data.registrations().ok_or(ENXIO)?.as_pinned_mut(),
            Self,
            PL061_GPIO_NR,
            None,
            dev,
            data.clone(),
            irq
        )?;

        dev_info!(data.dev, "PL061 GPIO chip registered\n");

        Ok(data)
    }
}

impl power::Operations for PL061Device {
    type Data = Arc<DeviceData>;

    fn suspend(data: ArcBorrow<'_, DeviceData>) -> Result {
        let mut inner = data.inner.lock();
        let pl061 = data.resources().ok_or(ENXIO)?;
        inner.csave_regs.gpio_data = 0;
        inner.csave_regs.gpio_dir = pl061.base.readb(GPIODIR);
        inner.csave_regs.gpio_is = pl061.base.readb(GPIOIS);
        inner.csave_regs.gpio_ibe = pl061.base.readb(GPIOIBE);
        inner.csave_regs.gpio_iev = pl061.base.readb(GPIOIEV);
        inner.csave_regs.gpio_ie = pl061.base.readb(GPIOIE);

        for offset in 0..PL061_GPIO_NR {
            if inner.csave_regs.gpio_dir & bit(offset) != 0 {
                if let Ok(v) = <Self as gpio::Chip>::get(data, offset.into()) {
                    inner.csave_regs.gpio_data |= (v as u8) << offset;
                }
            }
        }

        Ok(())
    }

    fn resume(data: ArcBorrow<'_, DeviceData>) -> Result {
        let inner = data.inner.lock();
        let pl061 = data.resources().ok_or(ENXIO)?;

        for offset in 0..PL061_GPIO_NR {
            if inner.csave_regs.gpio_dir & bit(offset) != 0 {
                let value = inner.csave_regs.gpio_data & bit(offset) != 0;
                let _ = <Self as gpio::Chip>::direction_output(data, offset.into(), value);
            } else {
                let _ = <Self as gpio::Chip>::direction_input(data, offset.into());
            }
        }

        pl061.base.writeb(inner.csave_regs.gpio_is, GPIOIS);
        pl061.base.writeb(inner.csave_regs.gpio_ibe, GPIOIBE);
        pl061.base.writeb(inner.csave_regs.gpio_iev, GPIOIEV);
        pl061.base.writeb(inner.csave_regs.gpio_ie, GPIOIE);

        Ok(())
    }

    fn freeze(data: ArcBorrow<'_, DeviceData>) -> Result {
        Self::suspend(data)
    }

    fn restore(data: ArcBorrow<'_, DeviceData>) -> Result {
        Self::resume(data)
    }
}

module_amba_driver! {
    type: PL061Device,
    name: "pl061_gpio",
    author: "Wedson Almeida Filho",
    license: "GPL",
}
