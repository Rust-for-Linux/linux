// SPDX-License-Identifier: GPL-2.0
#![allow(non_snake_case)]

//! ioctl() number definitions
//!
//! C header: [`include/asm-generic/ioctl.h`](../../../../include/asm-generic/ioctl.h)

/// Build an ioctl number, analogous to the C macro of the same name.
const fn _IOC(dir: u32, ty: u32, nr: u32, size: usize) -> u32 {
    core::assert!(dir <= bindings::_IOC_DIRMASK);
    core::assert!(ty <= bindings::_IOC_TYPEMASK);
    core::assert!(nr <= bindings::_IOC_NRMASK);
    core::assert!(size <= (bindings::_IOC_SIZEMASK as usize));

    (dir << bindings::_IOC_DIRSHIFT)
        | (ty << bindings::_IOC_TYPESHIFT)
        | (nr << bindings::_IOC_NRSHIFT)
        | ((size as u32) << bindings::_IOC_SIZESHIFT)
}

/// Build an ioctl number for an argumentless ioctl.
pub const fn _IO(ty: u32, nr: u32) -> u32 {
    _IOC(bindings::_IOC_NONE, ty, nr, 0)
}

/// Build an ioctl number for an read-only ioctl.
pub const fn _IOR<T>(ty: u32, nr: u32) -> u32 {
    _IOC(bindings::_IOC_READ, ty, nr, core::mem::size_of::<T>())
}

/// Build an ioctl number for an write-only ioctl.
pub const fn _IOW<T>(ty: u32, nr: u32) -> u32 {
    _IOC(bindings::_IOC_WRITE, ty, nr, core::mem::size_of::<T>())
}

/// Build an ioctl number for a read-write ioctl.
pub const fn _IOWR<T>(ty: u32, nr: u32) -> u32 {
    _IOC(
        bindings::_IOC_READ | bindings::_IOC_WRITE,
        ty,
        nr,
        core::mem::size_of::<T>(),
    )
}

/// Get the ioctl direction from an ioctl number.
pub const fn _IOC_DIR(nr: u32) -> u32 {
    (nr >> bindings::_IOC_DIRSHIFT) & bindings::_IOC_DIRMASK
}

/// Get the ioctl type from an ioctl number.
pub const fn _IOC_TYPE(nr: u32) -> u32 {
    (nr >> bindings::_IOC_TYPESHIFT) & bindings::_IOC_TYPEMASK
}

/// Get the ioctl number from an ioctl number.
pub const fn _IOC_NR(nr: u32) -> u32 {
    (nr >> bindings::_IOC_NRSHIFT) & bindings::_IOC_NRMASK
}

/// Get the ioctl size from an ioctl number.
pub const fn _IOC_SIZE(nr: u32) -> usize {
    ((nr >> bindings::_IOC_SIZESHIFT) & bindings::_IOC_SIZEMASK) as usize
}
