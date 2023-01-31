// SPDX-License-Identifier: GPL-2.0

//! Rust USB sample.

use kernel::{
    define_usb_id_table, device,
    error::code::*,
    file, init_static_sync,
    io_buffer::IoBufferWriter,
    miscdev, module_usb_driver, mutex_init, new_device_data,
    prelude::*,
    sync::{Arc, ArcBorrow, Mutex, NoWaitLock, UniqueArc},
    usb,
    xarray::XArray,
    GFP_ATOMIC, GFP_KERNEL,
};

const USB_SIMPLE_MINORS: u32 = 16;

init_static_sync! {
    static TABLE_MUTEX: Mutex<XArray<Arc<Mutex<UsbSimpleSyncData>>>> = XArray::new(0);
}

struct UsbSimpleFile;

struct UsbSimpleSyncData {
    buf: Vec<u8>,
    minor: usize,
}

#[vtable]
impl file::Operations for UsbSimpleFile {
    type Data = Arc<Mutex<UsbSimpleSyncData>>;

    fn open(_: &(), inode: &file::Inode, _: &file::File) -> Result<Self::Data> {
        let minors = TABLE_MUTEX.lock();
        let sbuf = minors.get(inode.minor() as usize).ok_or(EAGAIN)?;
        Ok(Arc::from(sbuf.borrow()))
    }

    fn read(
        shared: ArcBorrow<'_, Mutex<UsbSimpleSyncData>>,
        _: &file::File,
        writer: &mut impl IoBufferWriter,
        offset: u64,
    ) -> Result<usize> {
        if writer.is_empty() || offset != 0 {
            return Ok(0);
        }

        let sbuf = shared.lock();
        writer.write_slice(&sbuf.buf)?;
        Ok(sbuf.buf.len())
    }
}

struct UsbSimpleContext {
    sbuf: Arc<Mutex<UsbSimpleSyncData>>,
}

impl UsbSimpleContext {
    fn try_new(sbuf: UsbSimpleSyncData) -> Result<Arc<Self>> {
        // SAFETY: `mutex_init!` is called below.
        let mut sbuf = unsafe { Pin::from(UniqueArc::try_new(Mutex::new(sbuf))?) };
        let pinned = sbuf.as_mut();
        mutex_init!(pinned, "UsbSimpleContext::sbuf");
        Arc::try_new(Self { sbuf: sbuf.into() })
    }
}

struct UsbSimpleCompletion;

impl usb::Completion<Vec<u8>, Arc<UsbSimpleContext>> for UsbSimpleCompletion {
    fn complete(mut urb: UrbSimple) {
        let ctx = urb.context().unwrap();
        if let Some(mut sbuf) = ctx.sbuf.try_lock() {
            sbuf.buf.copy_from_slice(urb.borrow_transfer().unwrap());
        }
        urb.submit(GFP_ATOMIC).unwrap_or_default();
    }
}

type UsbSimpleRegistration = miscdev::Registration<UsbSimpleFile>;

type UrbSimple = usb::Urb<Vec<u8>, Arc<UsbSimpleContext>>;

struct UsbSimpleResources;

struct UsbSimpleData {
    urbs: Vec<UrbSimple>,
}

type DeviceData = device::Data<
    Vec<Pin<Box<UsbSimpleRegistration>>>,
    UsbSimpleResources,
    NoWaitLock<UsbSimpleData>,
>;

fn alloc_minors(urbs: &mut [UrbSimple], ports: usize) -> Result {
    let minors = TABLE_MUTEX.lock();

    let mut res = Ok(());
    let mut n = 0;
    for (i, ctx) in urbs
        .iter()
        .take(ports)
        .map_while(UrbSimple::context)
        .enumerate()
    {
        let mut sbuf = ctx.sbuf.lock();
        match minors.alloc_limits(Some(ctx.sbuf.clone()), 0, USB_SIMPLE_MINORS) {
            Ok(m) => sbuf.minor = m,
            Err(e) => {
                res = Err(e);
                n = i;
                break;
            }
        }
    }
    if res.is_err() {
        for ctx in urbs.iter().take(n).map_while(UrbSimple::context) {
            minors.remove(ctx.sbuf.lock().minor);
        }
    }
    res
}

fn release_minors(urbs: &mut [UrbSimple]) {
    let minors = TABLE_MUTEX.lock();

    for ctx in urbs.iter().map_while(UrbSimple::context) {
        minors.remove(ctx.sbuf.lock().minor);
    }
}

struct UsbSimpleDevice;

#[vtable]
impl usb::Driver for UsbSimpleDevice {
    type Data = Arc<DeviceData>;

    define_usb_id_table! {(), [(usb::DeviceId::default(), None)]}

    fn probe(intf: &mut usb::Interface, _id: Option<&()>) -> Result<Self::Data> {
        let dev = intf.to_usb_device();
        if dev.descriptor().id_vendor != *id_vendor.read()
            || dev.descriptor().id_product != *id_product.read()
        {
            return Err(ENODEV);
        }
        let mut in_edps = intf
            .cur_altsetting()
            .endpoints()
            .iter()
            .filter(|e| e.is_bulk_in());
        let epd_count = in_edps.by_ref().count();
        let mut urbs = Vec::try_with_capacity(epd_count)?;
        let mut regs = Vec::try_with_capacity(epd_count)?;
        for epd in in_edps {
            let mut urb = UrbSimple::try_new(0)?;
            let read_bulk = Vec::try_with_capacity(epd.maxp() as usize)?;
            let read_ctx = UsbSimpleContext::try_new(UsbSimpleSyncData {
                buf: Vec::try_with_capacity(epd.maxp() as usize)?,
                minor: 0,
            })?;
            urb.fill_bulk::<UsbSimpleCompletion>(
                &dev,
                dev.rcvbulkpipe(epd.b_endpoint_address as u32),
                Some(read_bulk),
                Some(read_ctx),
            );
            urb.submit(GFP_KERNEL).unwrap_or_default();
            regs.try_push(miscdev::Registration::<UsbSimpleFile>::new_pinned(
                fmt!("usbsimple"),
                (),
            )?)?;
            urbs.try_push(urb)?;
        }
        alloc_minors(&mut urbs, *num_ports.read() as usize)?;
        let data = new_device_data!(
            regs,
            UsbSimpleResources,
            NoWaitLock::new(UsbSimpleData { urbs }),
            "UsbSimple::Registrations"
        )?;
        Ok(data.into())
    }

    fn disconnect(_intf: &mut usb::Interface, data: &Self::Data) {
        if let Some(mut d) = data.try_lock() {
            release_minors(d.urbs.as_mut_slice());
            for urb in d.urbs.as_mut_slice() {
                urb.poison();
            }
        }
    }
}

module_usb_driver! {
    type: UsbSimpleDevice,
    name: "rust_usb_simple",
    author: "Martin Rodriguez Reboredo",
    description: "Rust USB sample",
    license: "GPL v2",
    params: {
        id_vendor: u16 {
            default: 0x0,
            permissions: 0,
            description: "USB Vendor ID to probe",
        },
        id_product: u16 {
            default: 0x0,
            permissions: 0,
            description: "USB Product ID to probe",
        },
        num_ports: u16 {
            default: 0,
            permissions: 0,
            description: "Number of ports to expose",
        },
    },
}
