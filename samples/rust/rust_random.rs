// SPDX-License-Identifier: GPL-2.0

//! Rust random device.
//!
//! Adapted from Alex Gaynor's original available at
//! <https://github.com/alex/just-use/blob/master/src/lib.rs>.

use kernel::{
    file::{self, File},
    io_buffer::{IoBufferReader, IoBufferWriter},
    prelude::*,
};

struct RandomFile;

impl file::Operations for RandomFile {
    kernel::declare_file_operations!(read, write, read_iter, write_iter);

    fn open(_data: &(), _file: &File) -> Result {
        Ok(())
    }

    fn read(_this: (), file: &File, buf: &mut impl IoBufferWriter, _: u64) -> Result<usize> {
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

    fn write(_this: (), _file: &File, buf: &mut impl IoBufferReader, _: u64) -> Result<usize> {
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
    name: b"rust_random",
    author: b"Rust for Linux Contributors",
    description: b"Just use /dev/urandom: Now with early-boot safety",
    license: b"GPL v2",
}
