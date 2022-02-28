// SPDX-License-Identifier: GPL-2.0

//! Rust misc device that reports debug information with a seq_file entry in
//! debugfs.

use kernel::prelude::*;
use kernel::{
    file::File,
    file_operations::FileOperations,
    io_buffer::IoBufferWriter,
    miscdev, seq_file,
    seq_file::SeqFileDebugFsDirEntry,
    sync::{Mutex, Ref, RefBorrow, UniqueRef},
};

module! {
    type: RustMiscdev,
    name: b"rust_seq_file",
    author: b"Rust for Linux Contributors",
    description: b"Sample Rust miscellaneous device with a debugfs entry using seq_file",
    license: b"GPL v2",
}

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
impl FileOperations for Token {
    type Wrapper = Ref<SharedState>;
    type OpenData = Ref<SharedState>;

    kernel::declare_file_operations!(read, write);

    fn open(shared: &Ref<SharedState>, _file: &File) -> Result<Self::Wrapper> {
        Ok(shared.clone())
    }

    fn read(
        shared: RefBorrow<'_, SharedState>,
        _: &File,
        data: &mut impl IoBufferWriter,
        offset: u64,
    ) -> Result<usize> {
        // Succeed if the caller doesn't provide a buffer or if not at the start.
        if data.is_empty() || offset != 0 {
            return Ok(0);
        }

        {
            let mut inner = shared.inner.lock();
            inner.read_count += 1;
        }

        // Write a one-byte 1 to the reader.
        data.write_slice(&[b'a'; 1])?;
        Ok(1)
    }
}

struct Log {
    read_id: usize,
}

impl core::fmt::Display for Log {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "rust_seq_file read log: {}", self.read_id)
    }
}

impl seq_file::SeqOperations for Token {
    type OpenData = Ref<SharedState>;
    type DataWrapper = Ref<SharedState>;
    type IteratorWrapper = Box<(usize, usize)>;
    type Item = Log;

    fn open<'a>(open_data: RefBorrow<'a, SharedState>) -> Result<Ref<SharedState>> {
        Ok(open_data.into())
    }

    fn start<'a>(data: RefBorrow<'a, SharedState>) -> Option<Self::IteratorWrapper> {
        let total = data.inner.lock().read_count;
        Box::try_new((total, 1)).ok()
    }

    fn next(iterator: &mut Self::IteratorWrapper) -> bool {
        let total = iterator.0;
        let current = iterator.1;
        if total == current {
            false
        } else {
            iterator.1 += 1;
            true
        }
    }

    fn current(iterator: &(usize, usize)) -> core::option::Option<Log> {
        let total = iterator.0;
        let current = iterator.1;
        if total >= current {
            Some(Log { read_id: current })
        } else {
            None
        }
    }
}

struct RustMiscdev {
    _dev: Pin<Box<miscdev::Registration<Token>>>,
    _debugfs: SeqFileDebugFsDirEntry<Token>,
}

impl KernelModule for RustMiscdev {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust seq_file sample (init)\n");

        let state = SharedState::try_new()?;
        let debugfs = seq_file::debugfs_create_file(fmt!("{name}"), state.clone())?;

        Ok(RustMiscdev {
            _dev: miscdev::Registration::new_pinned(fmt!("{name}"), state)?,
            _debugfs: debugfs,
        })
    }
}

impl Drop for RustMiscdev {
    fn drop(&mut self) {
        pr_info!("Rust seq_file sample (exit)\n");
    }
}
