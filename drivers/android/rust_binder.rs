// SPDX-License-Identifier: GPL-2.0

//! Binder -- the Android IPC mechanism.
//!
//! TODO: This module is a work in progress.

use kernel::{
    io_buffer::IoBufferWriter,
    linked_list::{GetLinks, GetLinksWrapped, Links},
    miscdev::Registration,
    prelude::*,
    str::CStr,
    sync::Arc,
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
    authors: ["Wedson Almeida Filho"],
    description: "Android Binder",
    license: "GPL",
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
    _reg: Pin<Box<Registration<process::Process>>>,
}

impl kernel::Module for BinderModule {
    fn init(name: &'static CStr, _module: &'static kernel::ThisModule) -> Result<Self> {
        let ctx = Context::new()?;
        let reg = Registration::new_pinned(fmt!("{name}"), ctx)?;
        Ok(Self { _reg: reg })
    }
}
