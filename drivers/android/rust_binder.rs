// SPDX-License-Identifier: GPL-2.0

//! Binder -- the Android IPC mechanism.
//!
//! TODO: This module is a work in progress.

#![no_std]
#![feature(global_asm, try_reserve, allocator_api, concat_idents)]

use alloc::{boxed::Box, sync::Arc};
use core::pin::Pin;
use kernel::{
    c_str,
    io_buffer::IoBufferWriter,
    linked_list::{GetLinks, GetLinksWrapped, Links},
    miscdev::Registration,
    prelude::*,
    user_ptr::UserSlicePtrWriter,
};

mod allocation;
mod context;
mod defs;
mod node;
mod process;
mod range_alloc;
mod thread;
mod transaction;

use {context::Context, thread::Thread};

module! {
    type: BinderModule,
    name: "rust_binder",
    author: "Wedson Almeida Filho",
    description: "Android Binder",
    license: "GPL v2",
}

enum Either<L, R> {
    Left(L),
    Right(R),
}

trait DeliverToRead {
    /// Performs work. Returns true if remaining work items in the queue should be processed
    /// immediately, or false if it should return to caller before processing additional work
    /// items.
    fn do_work(self: Arc<Self>, thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool>;

    /// Cancels the given work item. This is called instead of [`DeliverToRead::do_work`] when work
    /// won't be delivered.
    fn cancel(self: Arc<Self>) {}

    /// Returns the linked list links for the work item.
    fn get_links(&self) -> &Links<dyn DeliverToRead>;
}

struct DeliverToReadListAdapter {}

impl GetLinks for DeliverToReadListAdapter {
    type EntryType = dyn DeliverToRead;

    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType> {
        data.get_links()
    }
}

impl GetLinksWrapped for DeliverToReadListAdapter {
    type Wrapped = Arc<dyn DeliverToRead>;
}

struct DeliverCode {
    code: u32,
    links: Links<dyn DeliverToRead>,
}

impl DeliverCode {
    fn new(code: u32) -> Self {
        Self {
            code,
            links: Links::new(),
        }
    }
}

impl DeliverToRead for DeliverCode {
    fn do_work(self: Arc<Self>, _thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool> {
        writer.write(&self.code)?;
        Ok(true)
    }

    fn get_links(&self) -> &Links<dyn DeliverToRead> {
        &self.links
    }
}

const fn ptr_align(value: usize) -> usize {
    let size = core::mem::size_of::<usize>() - 1;
    (value + size) & !size
}

unsafe impl Sync for BinderModule {}

struct BinderModule {
    _reg: Pin<Box<Registration<Arc<Context>>>>,
}

impl KernelModule for BinderModule {
    fn init() -> Result<Self> {
        let pinned_ctx = Context::new()?;
        let ctx = unsafe { Pin::into_inner_unchecked(pinned_ctx) };
        let reg = Registration::<Arc<Context>>::new_pinned::<process::Process>(
            c_str!("rust_binder"),
            None,
            ctx,
        )?;
        Ok(Self { _reg: reg })
    }
}
