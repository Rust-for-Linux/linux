// SPDX-License-Identifier: GPL-2.0

//! Rust random device
//!
//! Adapted from Alex Gaynor's original available at
//! <https://github.com/alex/just-use/blob/master/src/lib.rs>.

#![no_std]
#![feature(allocator_api, global_asm)]

use kernel::{
    file::File,
    file_operations::FileOperations,
    io_buffer::{IoBufferReader, IoBufferWriter},
    prelude::*,
};

#[derive(Default)]
struct RandomFile;

impl FileOperations for RandomFile {
    kernel::declare_file_operations!(read, write, read_iter, write_iter);

    fn read<T: IoBufferWriter>(&self, file: &File, buf: &mut T, _offset: u64) -> Result<usize> {
        let total_len = buf.len();
        let mut chunkbuf = [0; 256];

        while !buf.is_empty() {
            let len = chunkbuf.len().min(buf.len());
            let chunk = &mut chunkbuf[0..len];

            if file.is_blocking() {
                kernel::random::getrandom(chunk)?;
            } else {
                kernel::random::getrandom_nonblock(chunk)?;
            }
            buf.write_slice(chunk)?;
        }
        Ok(total_len)
    }

    fn write<T: IoBufferReader>(&self, _file: &File, buf: &mut T, _offset: u64) -> Result<usize> {
        let total_len = buf.len();
        let mut chunkbuf = [0; 256];
        while !buf.is_empty() {
            let len = chunkbuf.len().min(buf.len());
            let chunk = &mut chunkbuf[0..len];
            buf.read_slice(chunk)?;
            kernel::random::add_randomness(chunk);
        }
        Ok(total_len)
    }
}

module_misc_device! {
    type: RandomFile,
    name: "rust_random",
    author: "Rust for Linux Contributors",
    description: "Just use /dev/urandom: Now with early-boot safety",
    license: "GPL v2",
}
