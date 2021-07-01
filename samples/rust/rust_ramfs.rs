// SPDX-License-Identifier: GPL-2.0

//! Rust fs sample

#![no_std]
#![feature(allocator_api, global_asm)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use kernel::fs::*;
use kernel::prelude::*;
use kernel::str::CStr;
use kernel::{c_str, treedescr};
use kernel::{file::File, file_operations::FileOperations, io_buffer::IoBufferWriter, Error};

module_fs! {
    type: Ramfs,
    name: b"rust_ramfs",
    author: b"Rust for Linux Contributors",
    description: b"Rust Ramfs",
    license: b"GPL v2",
}

struct Ramfs;

#[derive(Default)]
struct FopsA;

#[derive(Default)]
struct FopsB;

impl FileOperations for FopsA {
    type Wrapper = Box<Self>;

    kernel::declare_file_operations!(read);

    fn read<T: IoBufferWriter>(_this: &Self, _: &File, data: &mut T, offset: u64) -> Result<usize> {
        if data.is_empty() || offset != 0 {
            return Ok(0);
        }

        data.write_slice(b"This is a test file.\n")?;
        Ok(b"This is a test file.\n".len())
    }
}

impl FileOperations for FopsB {
    type Wrapper = Box<Self>;

    kernel::declare_file_operations!(read);

    fn read<T: IoBufferWriter>(_this: &Self, _: &File, data: &mut T, _offset: u64) -> Result<usize> {
        if data.is_empty() {
            return Err(Error::EINVAL);
        }

        data.write_slice(&[b'I'; 1])?;
        Ok(1)
    }
}

impl FileSystem for Ramfs {
    const MOUNT_TYPE: MountType = MountType::Single;

    fn fill_super(sb: &mut SuperBlock, _data: &CStr, _silent: i32) -> Result<()> {
        let desc = treedescr! {
            "testfile", FopsA, S_IRUSR | S_IROTH;
            "infiniteI", FopsB, S_IRUSR;
        };

        simple_fill_super(sb, 17, &desc)?;

        Ok(())
    }
}
