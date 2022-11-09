// SPDX-License-Identifier: GPL-2.0

//! Rust miscellaneous device sample.

use kernel::prelude::*;
use kernel::{
    file::{self, File},
    io_buffer::{IoBufferReader, IoBufferWriter},
    macros::pin_data,
    miscdev, new_condvar, new_mutex, pin_init,
    sync::{Arc, ArcBorrow, CondVar, Mutex},
};

module! {
    type: RustMiscdev,
    name: "rust_miscdev",
    author: "Rust for Linux Contributors",
    description: "Rust miscellaneous device sample",
    license: "GPL",
}

const MAX_TOKENS: usize = 3;

struct SharedStateInner {
    token_count: usize,
}

#[pin_data]
struct SharedState {
    #[pin]
    state_changed: CondVar,
    #[pin]
    inner: Mutex<SharedStateInner>,
}

impl SharedState {
    fn try_new() -> Result<Arc<Self>> {
        Arc::pin_init(pin_init!(Self {
            state_changed: new_condvar!("SharedState::state_changed"),
            inner: new_mutex!(SharedStateInner { token_count: 0 }, "SharedState::inner"),
        }))
    }
}

struct Token;
#[vtable]
impl file::Operations for Token {
    type Data = Arc<SharedState>;
    type OpenData = Arc<SharedState>;

    fn open(shared: &Arc<SharedState>, _file: &File) -> Result<Self::Data> {
        Ok(shared.clone())
    }

    fn read(
        shared: ArcBorrow<'_, SharedState>,
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

            // Wait until we are allowed to decrement the token count or a signal arrives.
            while inner.token_count == 0 {
                if shared.state_changed.wait(&mut inner) {
                    return Err(EINTR);
                }
            }

            // Consume a token.
            inner.token_count -= 1;
        }

        // Notify a possible writer waiting.
        shared.state_changed.notify_all();

        // Write a one-byte 1 to the reader.
        data.write_slice(&[1u8; 1])?;
        Ok(1)
    }

    fn write(
        shared: ArcBorrow<'_, SharedState>,
        _: &File,
        data: &mut impl IoBufferReader,
        _offset: u64,
    ) -> Result<usize> {
        {
            let mut inner = shared.inner.lock();

            // Wait until we are allowed to increment the token count or a signal arrives.
            while inner.token_count == MAX_TOKENS {
                if shared.state_changed.wait(&mut inner) {
                    return Err(EINTR);
                }
            }

            // Increment the number of token so that a reader can be released.
            inner.token_count += 1;
        }

        // Notify a possible reader waiting.
        shared.state_changed.notify_all();
        Ok(data.len())
    }
}

struct RustMiscdev {
    _dev: Pin<Box<miscdev::Registration<Token>>>,
}

impl kernel::Module for RustMiscdev {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("Rust miscellaneous device sample (init)\n");

        let state = SharedState::try_new()?;

        Ok(RustMiscdev {
            _dev: miscdev::Registration::new_pinned(fmt!("{name}"), state)?,
        })
    }
}

impl Drop for RustMiscdev {
    fn drop(&mut self) {
        pr_info!("Rust miscellaneous device sample (exit)\n");
    }
}
