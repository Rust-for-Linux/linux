use alloc::boxed::Box;
use core::default::Default;
use core::pin::Pin;
use core::result;
use core::result::Result::Ok;
use core::ffi::c_void;
use kernel::c_str;
use kernel::prelude::*;
use kernel::tty::*;
use kernel::file::File;
use kernel::bindings;
// out
const MCR_DTR: u32 = 0x01;
const MCR_RTS: u32 = 0x02;
const MCR_LOOP: u32 = 0x04;

// in
const MSR_CTS: u32= 0x10;
const MSR_CD: u32= 0x20;
const MSR_DSR: u32 = 0x40;
const MSR_RI: u32 = 0x80;

// modem lines
const TIOCM_LE: i32 = 0x001;      // line enable
const TIOCM_DTR: u32 = 0x002;     // data terminal ready
const TIOCM_RTS: u32 = 0x004;     // request to send
const TIOCM_ST: i32 = 0x010;      // secondary transmit
const TIOCM_SR: i32 = 0x020;      // secondary receive
const TIOCM_CTS: u32 = 0x040;     // clear to send
const TIOCM_CAR: u32 = 0x100;     // carrier detect
const TIOCM_CD: u32 = TIOCM_CAR;  // carrier detect (alias)
const TIOCM_RNG: u32 = 0x200;     // ring
const TIOCM_RI: u32= TIOCM_RNG;  // ring (alias)
const TIOCM_DSR: u32 = 0x400;     // data set ready
const TIOCM_OUT1: i32= 0x2000;
const TIOCM_OUT2: i32 = 0x4000;
const TIOCM_LOOP: u32 = 0x8000;


//for termios
const IGNBRK: u32 = 0o0000001;
const BRKINT: u32 = 0o0000002;
const IGNPAR: u32 = 0o0000004;
const PARMRK: u32 = 0o0000010;
const INPCK: u32 = 0o0000020;

fn relevant_iflag(iflag: Option<u32>) -> u32 {
    iflag.unwrap() & (IGNBRK | BRKINT | IGNPAR | PARMRK | INPCK)
}

struct TTYMethods;

#[vtable]
impl Tty0ttyMethods for TTYMethods {
    fn open(tty: &mut TtyStruct, _file: &File) -> Result<i32> {
        pr_info!("the outer open method called");
        let mut tty0tty: *mut Tty0ttySerial = core::ptr::null_mut();
        let mut msr: u32 = 0;
        let mut mcr: u32 = 0;
    
        tty.set_driver_data(core::ptr::null_mut());
        let index = tty.get_index() as usize;
        pr_info!("index:{}",index);
        tty0tty = unsafe { *TTY0TTY_TABLE.offset(index as isize) };
    
        if tty0tty.is_null() {
            pr_info!("the tty0tty is null");
            let tty0tty_box = match Box::try_new(Tty0ttySerial::new()) {
                Ok(boxed) => boxed,
                Err(_) => {
                    pr_err!("Failed to allocate memory for Tty0ttySerial");
                    return Err(Error::from_kernel_errno(-12)); // ENOMEM
                }
            };
    
            tty0tty = Box::into_raw(tty0tty_box);

            initialize_tty_semaphore(tty0tty);

            //here we didn't impl "tty0tty->open_count = 0;" because the new() method set opencount to 0 default
            unsafe { *TTY0TTY_TABLE.offset(index as isize) = tty0tty };
        }
    
        unsafe {
            (*TPORT.offset(index as isize)).tty = tty.to_ptr();
            (*tty.to_ptr()).port = &mut (*TPORT.offset(index as isize));
        }
    
        if index % 2 == 0 {
            let table_entry = unsafe { *TTY0TTY_TABLE.offset(index as isize + 1) };
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count() > 0 {
                    unsafe { *table_entry }.set_mcr(mcr);
                }
            }
        } else {
            let table_entry = unsafe { *TTY0TTY_TABLE.offset(index as isize - 1) };
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count() > 0 {
                    unsafe { *table_entry }.set_mcr(mcr);
                }
            }
        }
    
        if (mcr & MCR_RTS) == MCR_RTS {
            msr |= MSR_CTS;
        }
        
        if (mcr & MCR_DTR) == MCR_DTR {
            msr |= MSR_DSR | MSR_CD;
        }

        unsafe {
        (*tty0tty).set_msr(msr);
        (*tty0tty).set_mcr(0);
        }
        sema_down(tty0tty);

        tty.set_driver_data(tty0tty as *mut core::ffi::c_void);

        unsafe {
        (*tty0tty).set_tty(tty.to_ptr());
       (*tty0tty).add_open_count();
    }
       sema_up(tty0tty);
        pr_info!("open success");
        
        Ok(0)
    }
    

   fn close(tty: &mut TtyStruct,_file:&File) {
    pr_info!("the outer close method called");
    let tty0tty: *mut Tty0ttySerial=tty.get_driver_data() as *mut Tty0ttySerial;
    
    if !tty0tty.is_null(){
        let msr=0;
        let tty0tty_serial = Tty0ttySerial::from_raw(tty0tty);
        let index = ( unsafe { core::ptr::read(tty0tty_serial.get_tty()) } ).index;
       if index%2==0{
        let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize + 1) };
        if !table_entry.is_null() {
            if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                unsafe { *table_entry }.set_msr(msr);
            }
        }
       }else{
        let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize - 1) };
        if !table_entry.is_null() {
            if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                unsafe { *table_entry }.set_msr(msr);
            }
       }
    }

        sema_down(tty0tty);

        if Tty0ttySerial::from_raw(tty0tty).get_open_count()==0{
            sema_up(tty0tty);
            return;
        }
        else{
            unsafe { *tty0tty }.sub_open_count();
            sema_up(tty0tty);
            return;
        }
    }
}

    fn write(tty: &mut TtyStruct,buf:*const u8,count: i32)-> Result<i32> {
        pr_info!("the outer write method called");
        let tty0tty: *mut Tty0ttySerial=tty.get_driver_data() as *mut Tty0ttySerial;
        let mut retval=0;
        let mut ttyx=TtyStruct::default().to_ptr();

        if tty0tty.is_null(){
            return Err(Error::from_kernel_errno(-12));
        }

        sema_down(tty0tty);

        if Tty0ttySerial::from_raw(tty0tty).get_open_count()==0{
            sema_down(tty0tty);
            return Ok(retval);
        }
        
        let tty0tty_serial = Tty0ttySerial::from_raw(tty0tty);
        let index = ( safe_read(tty0tty_serial.get_tty())).index;
       if index%2==0{
        let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize + 1) };
        if !table_entry.is_null() {
            if Tty0ttySerial::from_raw(table_entry).get_open_count()>0{
                ttyx=Tty0ttySerial::from_raw(table_entry).get_tty();
            }
        }
       }else{
        let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize - 1) };
        if !table_entry.is_null() {
            if Tty0ttySerial::from_raw(table_entry).get_open_count()>0{
                ttyx=Tty0ttySerial::from_raw(table_entry).get_tty();
            }
       }
    }

    if !ttyx.is_null(){
        unsafe {
            bindings::tty_insert_flip_string_fixed_flag((*ttyx).port,buf,0,count.try_into().unwrap());
            bindings::tty_flip_buffer_push((*ttyx).port);
        }
        retval=count;
    }

   sema_up(tty0tty);
    pr_info!("write success");
        Ok(retval)
    }

    fn write_room(tty: &mut TtyStruct)->Result<i32> {
        pr_info!("the outer write_room method called");
        let mut room=0;
        let tty0tty: *mut Tty0ttySerial=tty.get_driver_data() as *mut Tty0ttySerial;
        if tty0tty.is_null(){
            return Err(Error::from_kernel_errno(-19))
        }

        sema_down(tty0tty);

         if Tty0ttySerial::from_raw(tty0tty).get_open_count()==0{
            sema_up(tty0tty);
            return Ok(room);
        }
        room=255;

        sema_up(tty0tty);
        Ok(0)
    }

    fn set_termios(tty:&mut TtyStruct,old_termios:&mut Ktermios){
        pr_info!("the outer set_termios method called");
        let mut cflag=tty.get_c_cflag();
        let mut iflag=tty.get_c_iflag();
        if !old_termios.to_ptr().is_null(){
            if (cflag==old_termios.get_c_cflag()) && (relevant_iflag(iflag) ==old_termios.get_c_iflag()){
                return;
            }
        }
    }

    fn tiocmget(tty:&mut TtyStruct)->Result<i32> {
        pr_info!("the outer tiocmget method is called");
        let mut result: i32=0;
        let tty0tty: *mut Tty0ttySerial=tty.get_driver_data() as *mut Tty0ttySerial;
        let msr=Tty0ttySerial::from_raw(tty0tty).get_msr();
        let mcr=Tty0ttySerial::from_raw(tty0tty).get_mcr();

         //NOTE:here have some type transfer problem
        result = (((mcr & MCR_DTR) != 0).then_some(TIOCM_DTR).unwrap_or(0) |
             ((mcr & MCR_RTS) != 0).then_some(TIOCM_RTS).unwrap_or(0) |
             ((mcr & MCR_LOOP) != 0).then_some(TIOCM_LOOP).unwrap_or(0) |
             ((msr & MSR_CTS) != 0).then_some(TIOCM_CTS).unwrap_or(0) |
             ((msr & MSR_CD) != 0).then_some(TIOCM_CAR).unwrap_or(0) |
             ((msr & MSR_RI) != 0).then_some(TIOCM_RI).unwrap_or(0) |
             ((msr & MSR_DSR) != 0).then_some(TIOCM_DSR).unwrap_or(0)) as i32;
            
        Ok(result)
    }

    fn tiocmset(tty:&mut TtyStruct,set:u32,clear:u32)->Result<i32> {
        pr_info!("the outer tiocmset method is called");
        let tty0tty: *mut Tty0ttySerial=tty.get_driver_data() as *mut Tty0ttySerial;
        let mut mcr=Tty0ttySerial::from_raw(tty0tty).get_mcr();
        let mut msr=0;

        let tty0tty_serial = Tty0ttySerial::from_raw(tty0tty);
        let index = ( unsafe { core::ptr::read(tty0tty_serial.get_tty()) } ).index;
        if index%2==0{
            let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize + 1) };
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                    msr=Tty0ttySerial::from_raw(tty0tty).msr;
                }
            }
           }else{
            let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize - 1) };
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                    msr=Tty0ttySerial::from_raw(tty0tty).msr;
                }
           }
        }

        if set & TIOCM_RTS != 0 {
            mcr |= MCR_RTS;
            msr |= MSR_CTS;
        }
        
        if set & TIOCM_DTR != 0 {
            mcr |= MCR_DTR;
            msr |= MSR_DSR;
            msr |= MSR_CD;
        }
        
        if clear & TIOCM_RTS != 0 {
            mcr &= !MCR_RTS;
            msr &= !MSR_CTS;
        }
        
        if clear & TIOCM_DTR != 0 {
            mcr &= !MCR_DTR;
            msr &= !MSR_DSR;
            msr &= !MSR_CD;
        }

        (unsafe { *tty0tty }).set_mcr(mcr);

        if index%2==0{
            let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize + 1) };
            pr_info!("--assign entry success---");
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                   (unsafe { *table_entry }).set_msr(msr);
                }
            }
           }else{
            let table_entry: *mut Tty0ttySerial = unsafe { *TTY0TTY_TABLE.offset(index as isize - 1) };
            if !table_entry.is_null() {
                if Tty0ttySerial::from_raw(tty0tty).get_open_count()>0{
                    (unsafe { *table_entry }).set_msr(msr);
                }
           }
        }

        Ok(0)
    }
    /* 
    fn ioctl_tiocgserial(tty:&mut TtyStruct,_cmd:i32,_arg:u64)->i32 {
        pr_info!("the outer ioctl_tiocgserial method is called");
    }

    fn ioctl_tiocgicount(tty:&mut TtyStruct,_cmd:i32,_arg:u64)->i32 {
        pr_info!("the outer ioctl_tiocgicount method is called");
    }

    fn ioctl_tiocmiwait(tty:&mut TtyStruct,_cmd:i32,_arg:u64)->i32 {
        pr_info!("the outer ioctl_tiocmiwait method is called");
    }*/
}

struct Tty0tty{
    tty: Pin<Box<Registration<TTYMethods>>>,
}

impl kernel::Module for Tty0tty {
    fn init(_name: &'static CStr, mut _module: &'static ThisModule) -> Result<Self> {
        let lock = _module.kernel_param_lock();
        pr_info!("the tty params:{}, {}",short.read(&lock),pairs.read(&lock));
        pr_info!("--------------------------\n");
        pr_info!("tty0tty init\n");
        pr_info!("--------------------------\n");
        let i:i32;
        let pair_count=2*pairs.read(&lock);

        let reg = Registration::new_pinned(_module,c_str!("tty_driver"))?;
        pr_info!("reg assign success\n");


        Ok(Tty0tty{tty:reg})
    }
}

impl Drop for Tty0tty {
    fn drop(&mut self) {
        pr_info!("tty0tty exit");
    }
}
module! {
    type: Tty0tty,
    name: "rust_tty",
    author: "lwz",
    description: "Rust tty module sample",
    license: "GPL",
    params: {
        short: i32 {
            default: 4,
            permissions: 0o644,
            description: "tty short param",
        },
        pairs: i32 {
            default: 4,
            permissions: 0o644,
            description: "tty pairs param",
        },
    },
}