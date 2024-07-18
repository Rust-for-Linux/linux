use crate::bindings;
use crate::error::{code::*, Error, Result};
use crate::c_str;
use crate::file::File;
use crate::prelude::*;
use alloc::boxed::Box;
use bindings::ktermios;
use bindings::semaphore;
use core::marker::{PhantomData, PhantomPinned};
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use macros::vtable;
use core::ptr;
use core::mem;
use core::cell::UnsafeCell;
//use core::sync::atomic::{AtomicPtr, Ordering};
use crate::sync::Mutex;
use crate::str::CStr;


const DRIVER_VERSION: &str = "v1.2";
const DRIVER_AUTHOR: &str = "Wenzhaoliao";
const DRIVER_DESC: &str = "tty0tty null modem driver";
const TTY0TTY_MAJOR: usize = 240;  // experimental range
const TTY0TTY_MINOR: usize = 16;

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

//for ioctl
const TIOCGSERIAL: u32 = 0x541E;
const TIOCMIWAIT: u32 = 0x545C;
const TIOCGICOUNT: u32 = 0x545D;

//for tty0tty_ioctl_tiocgserial
const ASYNC_SKIP_TEST: i32 = 0x40;   // Equivalent to (1 << 6)
const ASYNC_AUTO_IRQ: i32 = 0x80;    // Equivalent to (1 << 7)


#[derive(Clone, Copy)]
pub struct Tty0ttySerial {
    pub tty: *mut bindings::tty_struct, 
    pub open_count: i32, 
    pub sem:bindings::semaphore,

    pub msr: u32, 
    pub mcr: u32, 
    
    pub serial: bindings::serial_struct, 
    pub wait:bindings::wait_queue_head_t,
    pub icount: bindings::async_icount,
}

impl Tty0ttySerial {
    pub fn new() -> Self {
        Tty0ttySerial {
            tty: ptr::null_mut(),
            open_count: 0,
            sem:bindings::semaphore::default(),
            msr: 0,
            mcr: 0,
            serial: bindings::serial_struct::default(),
            wait:bindings::wait_queue_head::default(),
            icount: bindings::async_icount::default(),
        }
    }

    pub fn from_raw(ptr: *mut Tty0ttySerial) -> Self {
        unsafe {
            assert!(!ptr.is_null());
            ptr::read(ptr)
        }
    }

    // Getter for icount
    pub fn get_icount(&self) -> bindings::async_icount {
        self.icount
    }

    // Getter for open_count
    pub fn get_open_count(&self) -> i32 {
        self.open_count
    }

    // Setter for open_count
    pub fn set_open_count(&mut self, open_count: i32) {
        self.open_count = open_count;
    }

    // Getter for mcr
    pub fn get_mcr(&self) -> u32 {
        self.mcr
    }

    // Setter for mcr
    pub fn set_mcr(&mut self, mcr: u32) {
        self.mcr = mcr;
    }

    // Getter for mcr
    pub fn get_msr(&self) -> u32 {
        self.msr
    }

    // Setter for mcr
    pub fn set_msr(&mut self, msr: u32) {
        self.msr = msr;
    }

    pub fn set_tty(&mut self, tty: *mut bindings::tty_struct) {
        self.tty = tty;
    }

    pub fn get_tty(&self)->*mut bindings::tty_struct{
        self.tty
    }

    pub fn add_open_count(&mut self) {
        self.open_count += 1;
    }

    pub fn sub_open_count(&mut self) {
        self.open_count -= 1;
    }


}

pub fn initialize_tty_semaphore(tty0tty: *mut Tty0ttySerial) {
    assert!(!tty0tty.is_null(), "Pointer is null");
    unsafe {
        bindings::sema_init(&mut (*tty0tty).sem as *mut bindings::semaphore, 1);
    }
}

/*pub fn initialize_tty_semaphore(tty0tty: *mut Tty0ttySerial) {
    unsafe {
        bindings::sema_init(&mut (*tty0tty).sem as *mut bindings::semaphore, 1);
    }
}*/


pub fn sema_up(tty0tty: *mut Tty0ttySerial){
    unsafe {
        bindings::up(&mut (*tty0tty).sem as *mut bindings::semaphore);
    }
}

pub fn sema_down(tty0tty: *mut Tty0ttySerial){
    unsafe {
        bindings::down(&mut (*tty0tty).sem as *mut bindings::semaphore);
    }
}

pub fn safe_read(ptr: *mut bindings::tty_struct) ->bindings::tty_struct{
    unsafe {
        assert!(!ptr.is_null(), "Pointer is null");
        ptr::read(ptr)
    }
}

#[vtable]
pub trait Tty0ttyMethods {
    /// Corresponds to the kernel's `tty0tty_driver`'s `open` method field.
    fn open(_tty: &mut TtyStruct,_file:&File) -> Result<i32> {
        unreachable!("There should be a NULL in the open filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `close` method field.
    fn close(_tty: &mut TtyStruct,_file:&File) {
        unreachable!("There should be a NULL in the close filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `write` method field.
    fn write(_tty: &mut TtyStruct,buf:*const u8,count: i32)-> Result<i32> {
        unreachable!("There should be a NULL in the write filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `write_room` method field.
    fn write_room(_tty: &mut TtyStruct)->Result<i32>{
        unreachable!("There should be a NULL in the write_room filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `set_termios` method field.
    fn set_termios(_tty:&mut TtyStruct,_old_termios:&mut Ktermios){
        unreachable!("There should be a NULL in the set_termios filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `tiocmget` method field.
    fn tiocmget(_tty:&mut TtyStruct)->Result<i32>{
        unreachable!("There should be a NULL in the tiocmget filed of tty0tty_driver's");
    }

    /// Corresponds to the kernel's `tty0tty_driver`'s `tiocmset` method field.
    fn tiocmset(_tty:&mut TtyStruct,_set:u32,_clear:u32)->Result<i32>{
        unreachable!("There should be a NULL in the tiocmget filed of tty0tty_driver's");
    }

    /* 
    /// Corresponds to the kernel's `tty0tty_driver`'s `ioctl_tiocgserial` method field.
    fn ioctl_tiocgserial(_tty:&mut TtyStruct,_cmd:u32,_arg:u64)->i32{
        unreachable!("There should be a NULL in the ioctl_tiocgserial filed of tty0tty_driver's");
    }

    fn ioctl_tiocmiwait(_tty:&mut TtyStruct,_cmd:u32,_arg:u64)->i32{
        unreachable!("There should be a NULL in the ioctl_tiocgserial filed of tty0tty_driver's");
    }

    fn ioctl_tiocgicount(_tty:&mut TtyStruct,_cmd:u32,_arg:u64)->i32{
        unreachable!("There should be a NULL in the ioctl_tiocgicount filed of tty0tty_driver's");
    }*/
    
    /* 
    extern "C" fn tty0tty_ioctl(tty: *mut bindings::tty_struct,cmd:u32,arg:u64)->i32{
        match cmd {
            TIOCGSERIAL => tty0tty_ioctl_tiocgserial(tty, cmd, arg),
            TIOCMIWAIT => tty0tty_ioctl_tiocmiwait(tty, cmd, arg),
            TIOCGICOUNT => tty0tty_ioctl_tiocgicount(tty, cmd, arg),
            _ => {
                // Handle default case or unknown command
                // You can return an error or handle it as needed
                -515 // Example return value for an error
            }
        }
    }*/
}

pub static mut TPORT: *mut bindings::tty_port = core::ptr::null_mut();
pub static mut TTY0TTY_TABLE: *mut *mut Tty0ttySerial = ptr::null_mut();



#[derive(Clone, Copy)]
pub struct TtyStruct(*mut bindings::tty_struct);

impl Default for TtyStruct {
    fn default() -> Self {
        // 这里创建一个默认值的 TtyStruct
        TtyStruct(ptr::null_mut())
    }
}

impl TtyStruct {
    ///create a ttystruct instance from a pointer
    pub unsafe fn from_ptr(stu: *mut bindings::tty_struct) -> Self {
        TtyStruct(stu)
    }

    /// Access the raw pointer from an [`tty_struct`] instance.
    pub fn to_ptr(&mut self) -> *mut bindings::tty_struct {
        self.0
    }

    pub fn get_driver_data(&mut self) -> *mut core::ffi::c_void {
        unsafe { (*self.to_ptr()).driver_data }
    }

    pub fn set_driver_data(&mut self, data: *mut core::ffi::c_void) {
        unsafe { (*self.to_ptr()).driver_data = data; }
    }

    pub fn get_c_cflag(&mut self) ->  u32 {
        unsafe { (*self.to_ptr()).termios.c_cflag }
    }

    pub fn set_c_cflag(&mut self, cflag:  u32) {
        unsafe { (*self.to_ptr()).termios.c_cflag = cflag; }
    }

    /*pub fn get_c_iflag(&mut self) ->  u32 {
        unsafe { (*self.to_ptr()).termios.c_iflag }
    }*/

    pub fn get_c_iflag(&mut self) -> Option<u32> {
        if self.to_ptr().is_null() {
            None
        } else {
            unsafe { Some((*self.to_ptr()).termios.c_iflag) }
        }
    }

    pub fn set_c_iflag(&mut self, iflag:  u32) {
        unsafe { (*self.to_ptr()).termios.c_iflag = iflag; }
    }

    /// Get the index field.
    pub fn get_index(&mut self) -> i32 {
        unsafe { (*self.to_ptr()).index }
    }

    pub fn get_port(&mut self) -> *mut bindings::tty_port{
        unsafe { (*self.to_ptr()).port }
    }
}
//static mut TTY0TTY_TTY_DRIVER: *mut bindings::tty_driver = ptr::null_mut();

#[derive(Clone, Copy)]
pub struct Ktermios(*mut bindings::ktermios);

impl Ktermios{
    ///create a ttystruct instance from a pointer
    pub unsafe fn from_ptr(kms: *mut bindings::ktermios) -> Self {
        Ktermios(kms)
    }

    /// Access the raw pointer from an [`tty_struct`] instance.
    pub fn to_ptr(&mut self) -> *mut bindings::ktermios {
        self.0
    }

    pub fn get_c_iflag(&mut self) ->  u32 {
        unsafe { (*self.to_ptr()).c_iflag }
    }

    pub fn get_c_cflag(&mut self) ->  u32 {
        unsafe { (*self.to_ptr()).c_cflag }
    }
}

static mut SERIAL_OPS: bindings::tty_operations = bindings::tty_operations {
    open: None,
    close: None,
    write: None,
    write_room: None,
    ioctl: Some(tty0tty_ioctl as unsafe extern "C" fn(*mut bindings::tty_struct, u32, u64)->core::ffi::c_int),
    lookup: None,
    install: None,
    remove: None,
    shutdown: None,
    cleanup: None,
    put_char: None,
    flush_chars: None,
    chars_in_buffer: None,
    compat_ioctl: None,
    set_termios:None,
    throttle: None,
    unthrottle: None,
    stop: None,
    start: None,
    hangup: None,
    break_ctl: None,
    flush_buffer: None,
    set_ldisc: None,
    wait_until_sent: None,
    send_xchar: None,
    tiocmget: None,
    tiocmset: None,
    resize: None,
    get_icount: None,
    get_serial: None,
    set_serial: None,
    show_fdinfo: None,
    proc_show: None,
};


unsafe extern "C" fn tty0tty_open<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct, file: *mut bindings::file) -> core::ffi::c_int {
    let file_obj = File(UnsafeCell::new(unsafe { *file }));
    T::open(&mut TtyStruct(tty), &file_obj);
    0
}


extern "C" fn tty0tty_close<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct, file: *mut bindings::file) {
    let file_obj = File(UnsafeCell::new(unsafe { *file }));
    T::close(&mut TtyStruct(tty), &file_obj);
}

extern "C" fn tty0tty_write<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct, buf: *const u8, count: i32) -> core::ffi::c_int{
    T::write(&mut TtyStruct(tty), buf, count);
    0
}

extern "C" fn tty0tty_write_room<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct) -> core::ffi::c_uint{
    T::write_room(&mut TtyStruct(tty));
    0
}

extern "C" fn tty0tty_set_termios<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct,old_termios: *const bindings::ktermios){
    let termios = old_termios as *mut ktermios;
    T::set_termios(&mut TtyStruct(tty),&mut Ktermios(termios));
}

extern "C" fn tty0tty_tiocmget<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct)->i32{
    T::tiocmget(&mut TtyStruct(tty));
    0
}

extern "C" fn tty0tty_tiocmset<T:Tty0ttyMethods>(tty: *mut bindings::tty_struct,set:u32,clear:u32)->i32{
    T::tiocmset(&mut TtyStruct(tty),set,clear);
    0
}

extern "C" fn tty0tty_ioctl_tiocgserial(tty: *mut bindings::tty_struct,cmd:u32,arg:u64)->i32{
    unsafe {
        let tty0tty = (*tty).driver_data as *mut Tty0ttySerial;
        if cmd==TIOCGSERIAL{
            let mut tmp=bindings::serial_struct::default();

            if arg==0{
                return -14;
            }
    
            tmp.type_ = (*tty0tty).serial.type_;
            tmp.line = (*tty0tty).serial.line;
            tmp.port = (*tty0tty).serial.port;
            tmp.irq = (*tty0tty).serial.irq;
            tmp.flags = ASYNC_SKIP_TEST | ASYNC_AUTO_IRQ;
            tmp.xmit_fifo_size = (*tty0tty).serial.xmit_fifo_size;
            tmp.baud_base = (*tty0tty).serial.baud_base;
            tmp.close_delay = 5 * 250;
            tmp.closing_wait = 30 * 250;
            tmp.custom_divisor = (*tty0tty).serial.custom_divisor;
            tmp.hub6 = (*tty0tty).serial.hub6;
            tmp.io_type = (*tty0tty).serial.io_type;
        

            //TODO:we don't impl if copy to user here
            if bindings::copy_to_user(arg as *mut core::ffi::c_void, &tmp as *const bindings::serial_struct as *const core::ffi::c_void, mem::size_of::<bindings::serial_struct>() as u64) == 0{
                return -14;
            }
            return 0;
        }
    }
    
    //T::ioctl_tiocgserial(&mut TtyStruct,cmd,arg)
    -515
}

extern "C" fn tty0tty_ioctl_tiocmiwait(tty: *mut bindings::tty_struct,cmd:u32,arg:u64)->i32{
    //TODO:miss asyn part in rust
    //T::ioctl_tiocmiwait(&mut TtyStruct,cmd,arg);
    unsafe {
        let tty0tty = (*tty).driver_data as *mut Tty0ttySerial;
        
        if cmd==TIOCMIWAIT{
            //let wait=bindings::__WAITQUEUE_INITIALIZER(wait, bindings::get_current());
            let mut wait = bindings::wait_queue_entry {
                private: bindings::get_current() as *mut core::ffi::c_void,
                func: Some(bindings::default_wake_function as unsafe extern "C" fn(_, _, _, _) -> _),
                entry: bindings::list_head {
                    next: ptr::null_mut(),
                    prev: ptr::null_mut(),
                },
                flags: 0,
            };
            let mut cnow=bindings::async_icount::default();
            let mut cprev=bindings::async_icount::default();

            cprev=(*tty0tty).icount;

            loop{
                bindings::add_wait_queue( &mut (*tty0tty).wait as *mut bindings::wait_queue_head_t,&mut wait as *mut bindings::wait_queue_entry);
                //set_current_state(TASK_INTERRUPTIBLE);
                //smp_store_mb(get_current()->__state, (0x0001));
                AtomicU32::from((*bindings::get_current()).__state).store(0x0001, Ordering::Release);
                bindings::schedule();
                bindings::remove_wait_queue( &mut (*tty0tty).wait as *mut bindings::wait_queue_head_t,&mut wait as *mut bindings::wait_queue_entry);
                
                if bindings::signal_pending(bindings::get_current()) !=0 {
                    return -512;
                }
                
                cnow=(*tty0tty).icount;
                if cnow.rng == cprev.rng && cnow.dsr == cprev.dsr &&
                    cnow.dcd == cprev.dcd && cnow.cts == cprev.cts {
                    return -5; // no change => error
                }
                if ((arg &  TIOCM_RNG as u64) != 0 && cnow.rng != cprev.rng) ||
                    ((arg &  TIOCM_DSR as u64) != 0 && cnow.dsr != cprev.dsr) ||
                    ((arg & TIOCM_CD as u64) != 0 && cnow.dcd != cprev.dcd) ||
                    ((arg & TIOCM_CTS as u64) != 0 && cnow.cts != cprev.cts) {
                    return 0;
}
                

                cprev=cnow
            }
        }
    }

    -515
}

extern "C" fn tty0tty_ioctl_tiocgicount(tty: *mut bindings::tty_struct,cmd:u32,arg:u64)->i32{
    unsafe {
        let tty0tty = (*tty).driver_data as *mut Tty0ttySerial;
        
        if cmd==TIOCGSERIAL{
            let mut cnow=bindings::async_icount::default();
            cnow=(*tty0tty).icount;
            let mut icount=bindings::serial_icounter_struct::default();
            
            icount.cts = cnow.cts as i32;
		    icount.dsr = cnow.dsr as i32;
		    icount.rng = cnow.rng as i32;
		    icount.dcd = cnow.dcd as i32;
		    icount.rx = cnow.rx as i32;
		    icount.tx = cnow.tx as i32;
		    icount.frame = cnow.frame as i32;
		    icount.overrun = cnow.overrun as i32;
		    icount.parity = cnow.parity as i32;
		    icount.brk = cnow.brk as i32;
		    icount.buf_overrun = cnow.buf_overrun as i32;

            //TODO:we don't impl if copy to user here
            if bindings::copy_to_user(arg as *mut core::ffi::c_void, &icount as *const bindings::serial_icounter_struct as *const core::ffi::c_void, mem::size_of::<bindings::serial_struct>() as u64) == 0{
                return -14;
            }
            return 0;
        }
    }
    //T::ioctl_tiocgicount(&mut TtyStruct,cmd,arg);
    -515
}

extern "C" fn tty0tty_ioctl(tty: *mut bindings::tty_struct,cmd:u32,arg:u64)->core::ffi::c_int{
    match cmd {
        TIOCGSERIAL => tty0tty_ioctl_tiocgserial(tty, cmd, arg),
        TIOCMIWAIT => tty0tty_ioctl_tiocmiwait(tty, cmd, arg),
        TIOCGICOUNT => tty0tty_ioctl_tiocgicount(tty, cmd, arg),
        _ => {
            // Handle default case or unknown command
            // You can return an error or handle it as needed
            -515 // Example return value for an error
        }
    }
}

pub struct Registration<T:Tty0ttyMethods> {
    this_module: &'static crate::ThisModule,
    registered: bool,
    name: &'static CStr,
    tty_driver: UnsafeCell<bindings::tty_driver>,
    _p: PhantomData<T>,
}

impl<T:Tty0ttyMethods> Registration<T> {
    pub fn new(this_module: &'static crate::ThisModule, name: &'static CStr) -> Self {
        Self {
            this_module,
            registered: false,
            name,
            tty_driver: UnsafeCell::new(bindings::tty_driver::default()),
            _p: PhantomData,
        }
    }

    pub fn new_pinned(
        this_module: &'static crate::ThisModule,
        name: &'static CStr
    ) -> Result<Pin<Box<Self>>> {
        let mut registration = Pin::from(Box::try_new(Self::new(this_module, name))?);
        registration.as_mut().register()?;

        Ok(registration)
    }

    pub fn register(self: Pin<&mut Self>,) -> Result {
        // SAFETY: We must ensure that we never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        if this.registered {
            pr_err!("Driver already registered\n");
            // Already registered.
            return Err(EINVAL);
        }

        let pair_count = 8;
         let mut tport_vec: Vec<bindings::tty_port> = Vec::try_with_capacity((pair_count*mem::size_of::<bindings::tty_port>()).try_into().unwrap()).unwrap();
        for _ in 0..pair_count {
            tport_vec.try_push(unsafe { mem::zeroed() }).unwrap();
        }

        let mut table_vec: Vec<*mut Tty0ttySerial> = Vec::try_with_capacity((pair_count*mem::size_of::<Tty0ttySerial>()).try_into().unwrap()).unwrap();
        for _ in 0..pair_count {
            table_vec.try_push(ptr::null_mut()).unwrap();
        }
        unsafe {
            TPORT = Box::into_raw(tport_vec.try_into_boxed_slice().unwrap()) as *mut bindings::tty_port;
            //below is to solve the NULL pointer deference problem meet in the open method
            TTY0TTY_TABLE = Box::into_raw(table_vec.try_into_boxed_slice().unwrap()) as *mut *mut Tty0ttySerial;
        }

    
        let mut tty_driver = unsafe { &mut *this.tty_driver.get() };
        let mut tty_driver_mut_ptr: *mut bindings::tty_driver = tty_driver;
        tty_driver_mut_ptr =  unsafe { bindings::__tty_alloc_driver(pair_count as u32, this.this_module as *const _ as *mut _, 0) }  ;
        
        if  unsafe {bindings::IS_ERR(tty_driver_mut_ptr as *const core::ffi::c_void)}  {
            let err = unsafe { bindings::PTR_ERR(tty_driver_mut_ptr as *const core::ffi::c_void) };
            pr_info!("alloc failed: {}\n", err);
            return Err(Error::from_kernel_errno(err.try_into().unwrap()));
        }
        else {
            pr_info!("alloc successed with capability:{}",pair_count);
        }

        unsafe {
        (*tty_driver_mut_ptr).owner = this.this_module as *const _ as *mut _;
        (*tty_driver_mut_ptr).driver_name = "tty0tty".as_ptr() as *const i8;
        (*tty_driver_mut_ptr).name = "tnt".as_ptr() as *const i8;
        (*tty_driver_mut_ptr).major = 240;
        (*tty_driver_mut_ptr).minor_start = 16;
        (*tty_driver_mut_ptr).type_ = bindings::TTY_DRIVER_TYPE_SERIAL as i16;
        (*tty_driver_mut_ptr).subtype = bindings::SERIAL_TYPE_NORMAL as i16;
        (*tty_driver_mut_ptr).flags = (bindings::TTY_DRIVER_RESET_TERMIOS | bindings::TTY_DRIVER_REAL_RAW) as u64;
        (*tty_driver_mut_ptr).init_termios = unsafe { bindings::tty_std_termios };
        (*tty_driver_mut_ptr).init_termios.c_iflag = 0;
        (*tty_driver_mut_ptr).init_termios.c_oflag = 0;
        (*tty_driver_mut_ptr).init_termios.c_cflag = bindings::B38400 | bindings::CS8 | bindings::CREAD;
        (*tty_driver_mut_ptr).init_termios.c_lflag = 0;
        (*tty_driver_mut_ptr).init_termios.c_ispeed = 38400;
        (*tty_driver_mut_ptr).init_termios.c_ospeed = 38400;


        if T::HAS_OPEN {
            SERIAL_OPS.open = Some(tty0tty_open::<T>);
        }
        if T::HAS_CLOSE{
            SERIAL_OPS.close = Some(tty0tty_close::<T>);
        }
        if T::HAS_WRITE {
            SERIAL_OPS.write = Some(tty0tty_write::<T>);
        }
        if T::HAS_WRITE_ROOM{
            SERIAL_OPS.write_room=Some(tty0tty_write_room::<T>);
        }
        if T::HAS_SET_TERMIOS{
            SERIAL_OPS.set_termios=Some(tty0tty_set_termios::<T>);
        }
        if T::HAS_TIOCMGET{
            SERIAL_OPS.tiocmget=Some(tty0tty_tiocmget::<T>);
        }
        if T::HAS_TIOCMSET{
            SERIAL_OPS.tiocmset=Some(tty0tty_tiocmset::<T>);
        }
    

        bindings::tty_set_operations(tty_driver_mut_ptr,&SERIAL_OPS);
        //(*tty_driver_mut_ptr).ops = &SERIAL_OPS;
        
        for i in 0..pair_count{
            bindings::tty_port_init(TPORT.add(i.try_into().unwrap()));
            bindings::tty_port_link_device(TPORT.add(i.try_into().unwrap()),tty_driver_mut_ptr,i.try_into().unwrap());
            }
        }
      
        let retval = unsafe { bindings::tty_register_driver(tty_driver_mut_ptr) };
        if retval != 0 {
            this.registered = false;
            pr_info!("Failed to register tty driver, error code: {:?}\n", retval);
            unsafe { bindings::tty_driver_kref_put(tty_driver_mut_ptr) };
            return Err(Error::from_kernel_errno(retval));
        }
        pr_info!("Successfully registered tty driver\n");
        pr_info!("{} {} \n",DRIVER_DESC,DRIVER_VERSION);
        this.registered = true;

        Ok(())
    
}

}

impl<T:Tty0ttyMethods> Drop for Registration <T>{
    fn drop(&mut self) {
        let mut tty0tty: *mut Tty0ttySerial = core::ptr::null_mut();
        for i in 0..8{
            unsafe { 
                bindings::tty_port_destroy(TPORT.add(i.try_into().unwrap())); 
                bindings::tty_unregister_device(self.tty_driver.get(), i);
            }
        }
        unsafe {bindings::tty_unregister_driver(self.tty_driver.get())};
        pr_info!("register exit");
    }
}

unsafe impl<T:Tty0ttyMethods> Sync for Registration<T> {}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T:Tty0ttyMethods> Send for Registration<T> {}
