// SPDX-License-Identifier: GPL-2.0

//! Rust misc device that reports debug information with a seq_file entry in
//! debugfs.

use kernel::prelude::*;
use kernel::{
    debugfs::{debugfs_create, DebugFsDirectory},
    file::{self, File},
    io_buffer::IoBufferWriter,
    miscdev,
    str::CString,
    sync::{Mutex, Ref, RefBorrow, UniqueRef},
};

module! {
    type: RustMiscdev,
    name: "rust_debugfs",
    author: "Rust for Linux Contributors",
    description: "Sample Rust miscellaneous device with a debugfs entry",
    license: "GPL v2",
}

#[derive(Clone, Copy)]
struct SharedStateInner {
    read_count: usize,
}

struct SharedState {
    inner: Mutex<SharedStateInner>,
}

impl SharedState {
    fn try_new() -> Result<Ref<Self>> {
        let mut state = Pin::from(UniqueRef::try_new(Self {
            // SAFETY: `mutex_init!` is called below.
            inner: unsafe { Mutex::new(SharedStateInner { read_count: 0 }) },
        })?);

        // SAFETY: `inner` is pinned when `state` is.
        let pinned = unsafe { state.as_mut().map_unchecked_mut(|s| &mut s.inner) };
        kernel::mutex_init!(pinned, "SharedState::inner");

        Ok(state.into())
    }
}

struct Token;
#[vtable]
impl file::Operations for Token {
    type Data = Ref<SharedState>;
    type OpenData = Ref<SharedState>;

    fn open(shared: &Ref<SharedState>, _file: &File) -> Result<Self::Data> {
        Ok(shared.clone())
    }

    fn read(
        shared: RefBorrow<'_, SharedState>,
        _: &File,
        _data: &mut impl IoBufferWriter,
        _offset: u64,
    ) -> Result<usize> {
        let mut inner = shared.inner.lock();
        inner.read_count += 1;
        Ok(0)
    }
}

struct DebugFsToken;
#[vtable]
impl file::Operations for DebugFsToken {
    type OpenData = Ref<SharedState>;
    type Data = Ref<SharedState>;

    fn open(shared: &Ref<SharedState>, _file: &File) -> Result<Self::Data> {
        Ok(shared.clone())
    }

    fn read(
        shared: RefBorrow<'_, SharedState>,
        _: &File,
        data: &mut impl IoBufferWriter,
        offset: u64,
    ) -> Result<usize> {
        if offset != 0 {
            return Ok(0);
        }

        let read_count = shared.inner.lock().read_count;
        let string = CString::try_from_fmt(fmt!("Debugfs file read count: {}\n", read_count))?;
        data.write_slice(string.as_bytes())?;
        Ok(string.len())
    }
}

struct RustMiscdev {
    _dev: Pin<Box<miscdev::Registration<Token>>>,
    _debugfs_dir: DebugFsDirectory,
}

impl kernel::Module for RustMiscdev {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust debugfs sample (init)\n");

        let state = SharedState::try_new()?;
        let dir_name = CString::try_from_fmt(fmt!("{name}_debug"))?;
        let mut debugfs_dir = DebugFsDirectory::create(&dir_name, None)?;
        let file_name = CString::try_from_fmt(fmt!("{name}"))?;
        let _debugfs_file =
            debugfs_create::<DebugFsToken>(&file_name, Some(&mut debugfs_dir), state.clone())?;

        Ok(RustMiscdev {
            _dev: miscdev::Registration::new_pinned(fmt!("{name}"), state)?,
            _debugfs_dir: debugfs_dir,
        })
    }
}

impl Drop for RustMiscdev {
    fn drop(&mut self) {
        pr_info!("Rust debugfs sample (exit)\n");
    }
}
