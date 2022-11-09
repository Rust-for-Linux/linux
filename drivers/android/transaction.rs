// SPDX-License-Identifier: GPL-2.0

use core::sync::atomic::{AtomicBool, Ordering};
use kernel::{
    bindings,
    file::{File, FileDescriptorReservation},
    io_buffer::IoBufferWriter,
    linked_list::List,
    linked_list::{GetLinks, Links},
    macros::pinned_drop,
    new_spinlock,
    prelude::*,
    sync::{Arc, SpinLock, UniqueArc},
    user_ptr::UserSlicePtrWriter,
    Either, ScopeGuard,
};

use crate::{
    defs::*,
    node::NodeRef,
    process::Process,
    ptr_align,
    thread::{BinderResult, Thread},
    DeliverToRead,
};

struct TransactionInner {
    file_list: List<Box<FileInfo>>,
}

#[pin_data(PinnedDrop)]
pub(crate) struct Transaction {
    #[pin]
    inner: SpinLock<TransactionInner>,
    // TODO: Node should be released when the buffer is released.
    node_ref: Option<NodeRef>,
    stack_next: Option<Arc<Transaction>>,
    pub(crate) from: Arc<Thread>,
    to: Arc<Process>,
    free_allocation: AtomicBool,
    code: u32,
    pub(crate) flags: u32,
    data_size: usize,
    offsets_size: usize,
    data_address: usize,
    links: Links<dyn DeliverToRead>,
}

impl Transaction {
    pub(crate) fn new(
        node_ref: NodeRef,
        stack_next: Option<Arc<Transaction>>,
        from: &Arc<Thread>,
        tr: &BinderTransactionData,
    ) -> BinderResult<Arc<Self>> {
        let allow_fds = node_ref.node.flags & FLAT_BINDER_FLAG_ACCEPTS_FDS != 0;
        let to = node_ref.node.owner.clone();
        let mut alloc = from.copy_transaction_data(&to, tr, allow_fds)?;
        let data_address = alloc.ptr;
        let file_list = alloc.take_file_list();
        alloc.keep_alive();
        let tr = UniqueArc::pin_init(pin_init!(Self {
            inner: new_spinlock!(TransactionInner { file_list }, "Transaction::inner"),
            node_ref: Some(node_ref),
            stack_next,
            from: from.clone(),
            to,
            code: tr.code,
            flags: tr.flags,
            data_size: tr.data_size as usize,
            data_address,
            offsets_size: tr.offsets_size as usize,
            links: Links::new(),
            free_allocation: AtomicBool::new(true),
        }))?;
        Ok(tr.into())
    }

    pub(crate) fn new_reply(
        from: &Arc<Thread>,
        to: Arc<Process>,
        tr: &BinderTransactionData,
        allow_fds: bool,
    ) -> BinderResult<Arc<Self>> {
        let mut alloc = from.copy_transaction_data(&to, tr, allow_fds)?;
        let data_address = alloc.ptr;
        let file_list = alloc.take_file_list();
        alloc.keep_alive();
        let tr = UniqueArc::pin_init(pin_init!(Self {
            inner: new_spinlock!(TransactionInner { file_list }, "Transaction::inner"),
            node_ref: None,
            stack_next: None,
            from: from.clone(),
            to,
            code: tr.code,
            flags: tr.flags,
            data_size: tr.data_size as usize,
            data_address,
            offsets_size: tr.offsets_size as usize,
            links: Links::new(),
            free_allocation: AtomicBool::new(true),
        }))?;
        Ok(tr.into())
    }

    /// Determines if the transaction is stacked on top of the given transaction.
    pub(crate) fn is_stacked_on(&self, onext: &Option<Arc<Self>>) -> bool {
        match (&self.stack_next, onext) {
            (None, None) => true,
            (Some(stack_next), Some(next)) => Arc::ptr_eq(stack_next, next),
            _ => false,
        }
    }

    /// Returns a pointer to the next transaction on the transaction stack, if there is one.
    pub(crate) fn clone_next(&self) -> Option<Arc<Self>> {
        let next = self.stack_next.as_ref()?;
        Some(next.clone())
    }

    /// Searches in the transaction stack for a thread that belongs to the target process. This is
    /// useful when finding a target for a new transaction: if the node belongs to a process that
    /// is already part of the transaction stack, we reuse the thread.
    fn find_target_thread(&self) -> Option<Arc<Thread>> {
        let process = &self.node_ref.as_ref()?.node.owner;

        let mut it = &self.stack_next;
        while let Some(transaction) = it {
            if Arc::ptr_eq(&transaction.from.process, process) {
                return Some(transaction.from.clone());
            }
            it = &transaction.stack_next;
        }
        None
    }

    /// Searches in the transaction stack for a transaction originating at the given thread.
    pub(crate) fn find_from(&self, thread: &Thread) -> Option<Arc<Transaction>> {
        let mut it = &self.stack_next;
        while let Some(transaction) = it {
            if core::ptr::eq(thread, transaction.from.as_ref()) {
                return Some(transaction.clone());
            }

            it = &transaction.stack_next;
        }
        None
    }

    /// Submits the transaction to a work queue. Use a thread if there is one in the transaction
    /// stack, otherwise use the destination process.
    pub(crate) fn submit(self: Arc<Self>) -> BinderResult {
        if let Some(thread) = self.find_target_thread() {
            thread.push_work(self)
        } else {
            let process = self.to.clone();
            process.push_work(self)
        }
    }

    /// Prepares the file list for delivery to the caller.
    fn prepare_file_list(&self) -> Result<List<Box<FileInfo>>> {
        // Get list of files that are being transferred as part of the transaction.
        let mut file_list = core::mem::replace(&mut self.inner.lock().file_list, List::new());

        // If the list is non-empty, prepare the buffer.
        if !file_list.is_empty() {
            let alloc = self.to.buffer_get(self.data_address).ok_or(ESRCH)?;
            let cleanup = ScopeGuard::new(|| {
                self.free_allocation.store(false, Ordering::Relaxed);
            });

            let mut it = file_list.cursor_front_mut();
            while let Some(file_info) = it.current() {
                let reservation = FileDescriptorReservation::new(bindings::O_CLOEXEC)?;
                alloc.write(file_info.buffer_offset, &reservation.reserved_fd())?;
                file_info.reservation = Some(reservation);
                it.move_next();
            }

            alloc.keep_alive();
            cleanup.dismiss();
        }

        Ok(file_list)
    }
}

impl DeliverToRead for Transaction {
    fn do_work(self: Arc<Self>, thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool> {
        // TODO: Initialise the following fields from `tr`:
        //   - `pub sender_pid: pid_t`.
        //   - `pub sender_euid: uid_t`.

        let send_failed_reply = ScopeGuard::new(|| {
            if self.node_ref.is_some() && self.flags & TF_ONE_WAY == 0 {
                let reply = Either::Right(BR_FAILED_REPLY);
                self.from.deliver_reply(reply, &self);
            }
        });
        let mut file_list = if let Ok(list) = self.prepare_file_list() {
            list
        } else {
            // On failure to process the list, we send a reply back to the sender and ignore the
            // transaction on the recipient.
            return Ok(true);
        };

        let mut tr = BinderTransactionData::default();

        if let Some(nref) = &self.node_ref {
            let (ptr, cookie) = nref.node.get_id();
            tr.target.ptr = ptr as _;
            tr.cookie = cookie as _;
        };

        tr.code = self.code;
        tr.flags = self.flags;
        tr.data_size = self.data_size as _;
        tr.data.ptr.buffer = self.data_address as _;
        tr.offsets_size = self.offsets_size as _;
        if tr.offsets_size > 0 {
            tr.data.ptr.offsets = (self.data_address + ptr_align(self.data_size)) as _;
        }

        let code = if self.node_ref.is_none() {
            BR_REPLY
        } else {
            BR_TRANSACTION
        };

        // Write the transaction code and data to the user buffer.
        writer.write(&code)?;
        writer.write(&tr)?;

        // Dismiss the completion of transaction with a failure. No failure paths are allowed from
        // here on out.
        send_failed_reply.dismiss();

        // Commit all files.
        {
            let mut it = file_list.cursor_front_mut();
            while let Some(file_info) = it.current() {
                if let Some(reservation) = file_info.reservation.take() {
                    if let Some(file) = file_info.file.take() {
                        reservation.commit(file);
                    }
                }

                it.move_next();
            }
        }

        // When `drop` is called, we don't want the allocation to be freed because it is now the
        // user's reponsibility to free it.
        //
        // `drop` is guaranteed to see this relaxed store because `Arc` guarantess that everything
        // that happens when an object is referenced happens-before the eventual `drop`.
        self.free_allocation.store(false, Ordering::Relaxed);

        // When this is not a reply and not an async transaction, update `current_transaction`. If
        // it's a reply, `current_transaction` has already been updated appropriately.
        if self.node_ref.is_some() && tr.flags & TF_ONE_WAY == 0 {
            thread.set_current_transaction(self);
        }

        Ok(false)
    }

    fn cancel(self: Arc<Self>) {
        let reply = Either::Right(BR_DEAD_REPLY);
        self.from.deliver_reply(reply, &self);
    }

    fn get_links(&self) -> &Links<dyn DeliverToRead> {
        &self.links
    }
}

#[pinned_drop]
impl PinnedDrop for Transaction {
    fn drop(self: Pin<&mut Self>) {
        if self.free_allocation.load(Ordering::Relaxed) {
            self.to.buffer_get(self.data_address);
        }
    }
}

pub(crate) struct FileInfo {
    links: Links<FileInfo>,

    /// The file for which a descriptor will be created in the recipient process.
    file: Option<ARef<File>>,

    /// The file descriptor reservation on the recipient process.
    reservation: Option<FileDescriptorReservation>,

    /// The offset in the buffer where the file descriptor is stored.
    buffer_offset: usize,
}

impl FileInfo {
    pub(crate) fn new(file: ARef<File>, buffer_offset: usize) -> Self {
        Self {
            file: Some(file),
            reservation: None,
            buffer_offset,
            links: Links::new(),
        }
    }
}

impl GetLinks for FileInfo {
    type EntryType = Self;

    fn get_links(data: &Self::EntryType) -> &Links<Self::EntryType> {
        &data.links
    }
}
