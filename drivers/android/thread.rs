// SPDX-License-Identifier: GPL-2.0

use core::{
    alloc::AllocError,
    mem::size_of,
    sync::atomic::{AtomicU32, Ordering},
};
use kernel::{
    bindings,
    file::{File, PollTable},
    io_buffer::{IoBufferReader, IoBufferWriter},
    linked_list::{GetLinks, Links, List},
    new_condvar, new_spinlock,
    prelude::*,
    security,
    sync::{Arc, CondVar, SpinLock},
    user_ptr::{UserSlicePtr, UserSlicePtrWriter},
    Either,
};

use crate::{
    allocation::{Allocation, AllocationView},
    defs::*,
    process::{AllocationInfo, Process},
    ptr_align,
    transaction::{FileInfo, Transaction},
    DeliverCode, DeliverToRead, DeliverToReadListAdapter,
};

pub(crate) type BinderResult<T = ()> = core::result::Result<T, BinderError>;

pub(crate) struct BinderError {
    pub(crate) reply: u32,
}

impl BinderError {
    pub(crate) fn new_failed() -> Self {
        Self {
            reply: BR_FAILED_REPLY,
        }
    }

    pub(crate) fn new_dead() -> Self {
        Self {
            reply: BR_DEAD_REPLY,
        }
    }
}

impl From<Error> for BinderError {
    fn from(_: Error) -> Self {
        Self::new_failed()
    }
}

impl From<AllocError> for BinderError {
    fn from(_: AllocError) -> Self {
        Self::new_failed()
    }
}

const LOOPER_REGISTERED: u32 = 0x01;
const LOOPER_ENTERED: u32 = 0x02;
const LOOPER_EXITED: u32 = 0x04;
const LOOPER_INVALID: u32 = 0x08;
const LOOPER_WAITING: u32 = 0x10;
const LOOPER_POLL: u32 = 0x20;

struct InnerThread {
    /// Determines the looper state of the thread. It is a bit-wise combination of the constants
    /// prefixed with `LOOPER_`.
    looper_flags: u32,

    /// Determines if thread is dead.
    is_dead: bool,

    /// Work item used to deliver error codes to the thread that started a transaction. When set to
    /// `Some(x)`, it will hold the only reference to the object so that it can update the error
    /// code to be delivered before queuing it.
    reply_work: Option<Arc<ThreadError>>,

    /// Work item used to deliver error codes to the current thread. When set to `Some(x)`, it will
    /// hold the only reference to the object so that it can update the error code to be delivered
    /// before queuing.
    return_work: Option<Arc<ThreadError>>,

    /// Determines whether the work list below should be processed. When set to false, `work_list`
    /// is treated as if it were empty.
    process_work_list: bool,
    work_list: List<DeliverToReadListAdapter>,
    current_transaction: Option<Arc<Transaction>>,
}

impl InnerThread {
    fn new() -> Self {
        Self {
            looper_flags: 0,
            is_dead: false,
            process_work_list: false,
            work_list: List::new(),
            current_transaction: None,
            return_work: None,
            reply_work: None,
        }
    }

    fn set_reply_work(&mut self, reply_work: Arc<ThreadError>) {
        self.reply_work = Some(reply_work);
    }

    fn push_reply_work(&mut self, code: u32) {
        let work = self.reply_work.take();
        self.push_existing_work(work, code);
    }

    fn set_return_work(&mut self, return_work: Arc<ThreadError>) {
        self.return_work = Some(return_work);
    }

    fn push_return_work(&mut self, code: u32) {
        let work = self.return_work.take();
        self.push_existing_work(work, code);
    }

    fn push_existing_work(&mut self, owork: Option<Arc<ThreadError>>, code: u32) {
        // TODO: Write some warning when the following fails. It should not happen, and
        // if it does, there is likely something wrong.
        if let Some(work) = owork {
            // `error_code` is written to with relaxed semantics because the queue onto which it is
            // being inserted is protected by a lock. The release barrier when the lock is released
            // by the caller matches with the acquire barrier of the future reader to guarantee
            // that `error_code` is visible.
            work.error_code.store(code, Ordering::Relaxed);
            self.push_work(work);
        }
    }

    fn pop_work(&mut self) -> Option<Arc<dyn DeliverToRead>> {
        if !self.process_work_list {
            return None;
        }

        let ret = self.work_list.pop_front();
        // Once the queue is drained, we stop processing it until a non-deferred item is pushed
        // again onto it.
        self.process_work_list = !self.work_list.is_empty();
        ret
    }

    fn push_work_deferred(&mut self, work: Arc<dyn DeliverToRead>) {
        self.work_list.push_back(work);
    }

    fn push_work(&mut self, work: Arc<dyn DeliverToRead>) {
        self.push_work_deferred(work);
        self.process_work_list = true;
    }

    fn has_work(&self) -> bool {
        self.process_work_list && !self.work_list.is_empty()
    }

    /// Fetches the transaction the thread can reply to. If the thread has a pending transaction
    /// (that it could respond to) but it has also issued a transaction, it must first wait for the
    /// previously-issued transaction to complete.
    fn pop_transaction_to_reply(&mut self, thread: &Thread) -> Result<Arc<Transaction>> {
        let transaction = self.current_transaction.take().ok_or(EINVAL)?;

        if core::ptr::eq(thread, transaction.from.as_ref()) {
            self.current_transaction = Some(transaction);
            return Err(EINVAL);
        }

        // Find a new current transaction for this thread.
        self.current_transaction = transaction.find_from(thread);
        Ok(transaction)
    }

    fn pop_transaction_replied(&mut self, transaction: &Arc<Transaction>) -> bool {
        match self.current_transaction.take() {
            None => false,
            Some(old) => {
                if !Arc::ptr_eq(transaction, &old) {
                    self.current_transaction = Some(old);
                    return false;
                }
                self.current_transaction = old.clone_next();
                true
            }
        }
    }

    fn looper_enter(&mut self) {
        self.looper_flags |= LOOPER_ENTERED;
        if self.looper_flags & LOOPER_REGISTERED != 0 {
            self.looper_flags |= LOOPER_INVALID;
        }
    }

    fn looper_register(&mut self, valid: bool) {
        self.looper_flags |= LOOPER_REGISTERED;
        if !valid || self.looper_flags & LOOPER_ENTERED != 0 {
            self.looper_flags |= LOOPER_INVALID;
        }
    }

    fn looper_exit(&mut self) {
        self.looper_flags |= LOOPER_EXITED;
    }

    /// Determines whether the thread is part of a pool, i.e., if it is a looper.
    fn is_looper(&self) -> bool {
        self.looper_flags & (LOOPER_ENTERED | LOOPER_REGISTERED) != 0
    }

    /// Determines whether the thread should attempt to fetch work items from the process queue
    /// (when its own queue is empty). This is case when the thread is not part of a transaction
    /// stack and it is registered as a looper.
    fn should_use_process_work_queue(&self) -> bool {
        self.current_transaction.is_none() && self.is_looper()
    }

    fn poll(&mut self) -> u32 {
        self.looper_flags |= LOOPER_POLL;
        if self.has_work() {
            bindings::POLLIN
        } else {
            0
        }
    }
}

#[pin_project]
pub(crate) struct Thread {
    pub(crate) id: i32,
    pub(crate) process: Arc<Process>,
    #[pin]
    inner: SpinLock<InnerThread>,
    #[pin]
    work_condvar: CondVar,
    links: Links<Thread>,
}

impl Thread {
    pub(crate) fn new(id: i32, process: Arc<Process>) -> Result<Arc<Self>> {
        let return_work = Arc::try_new(ThreadError::new(InnerThread::set_return_work))?;
        let reply_work = Arc::try_new(ThreadError::new(InnerThread::set_reply_work))?;
        let thread = Arc::pin_init::<core::convert::Infallible>(pin_init!(Self {
            id,
            process,
            inner: new_spinlock!(InnerThread::new(), "Thread::inner"),
            work_condvar: new_condvar!("Thread::work_condvar"),
            links: Links::new(),
        }))?;

        {
            let mut inner = thread.inner.lock();
            inner.set_reply_work(reply_work);
            inner.set_return_work(return_work);
        }

        Ok(thread)
    }

    pub(crate) fn set_current_transaction(&self, transaction: Arc<Transaction>) {
        self.inner.lock().current_transaction = Some(transaction);
    }

    /// Attempts to fetch a work item from the thread-local queue. The behaviour if the queue is
    /// empty depends on `wait`: if it is true, the function waits for some work to be queued (or a
    /// signal); otherwise it returns indicating that none is available.
    fn get_work_local(self: &Arc<Self>, wait: bool) -> Result<Arc<dyn DeliverToRead>> {
        // Try once if the caller does not want to wait.
        if !wait {
            return self.inner.lock().pop_work().ok_or(EAGAIN);
        }

        // Loop waiting only on the local queue (i.e., not registering with the process queue).
        let mut inner = self.inner.lock();
        loop {
            if let Some(work) = inner.pop_work() {
                return Ok(work);
            }

            inner.looper_flags |= LOOPER_WAITING;
            let signal_pending = self.work_condvar.wait(&mut inner);
            inner.looper_flags &= !LOOPER_WAITING;

            if signal_pending {
                return Err(ERESTARTSYS);
            }
        }
    }

    /// Attempts to fetch a work item from the thread-local queue, falling back to the process-wide
    /// queue if none is available locally.
    ///
    /// This must only be called when the thread is not participating in a transaction chain. If it
    /// is, the local version (`get_work_local`) should be used instead.
    fn get_work(self: &Arc<Self>, wait: bool) -> Result<Arc<dyn DeliverToRead>> {
        // Try to get work from the thread's work queue, using only a local lock.
        {
            let mut inner = self.inner.lock();
            if let Some(work) = inner.pop_work() {
                return Ok(work);
            }
        }

        // If the caller doesn't want to wait, try to grab work from the process queue.
        //
        // We know nothing will have been queued directly to the thread queue because it is not in
        // a transaction and it is not in the process' ready list.
        if !wait {
            return self.process.get_work().ok_or(EAGAIN);
        }

        // Get work from the process queue. If none is available, atomically register as ready.
        let reg = match self.process.get_work_or_register(self) {
            Either::Left(work) => return Ok(work),
            Either::Right(reg) => reg,
        };

        let mut inner = self.inner.lock();
        loop {
            if let Some(work) = inner.pop_work() {
                return Ok(work);
            }

            inner.looper_flags |= LOOPER_WAITING;
            let signal_pending = self.work_condvar.wait(&mut inner);
            inner.looper_flags &= !LOOPER_WAITING;

            if signal_pending {
                // A signal is pending. We need to pull the thread off the list, then check the
                // state again after it's off the list to ensure that something was not queued in
                // the meantime. If something has been queued, we just return it (instead of the
                // error).
                drop(inner);
                drop(reg);
                return self.inner.lock().pop_work().ok_or(ERESTARTSYS);
            }
        }
    }

    pub(crate) fn push_work(&self, work: Arc<dyn DeliverToRead>) -> BinderResult {
        {
            let mut inner = self.inner.lock();
            if inner.is_dead {
                return Err(BinderError::new_dead());
            }
            inner.push_work(work);
        }
        self.work_condvar.notify_one();
        Ok(())
    }

    /// Attempts to push to given work item to the thread if it's a looper thread (i.e., if it's
    /// part of a thread pool) and is alive. Otherwise, push the work item to the process instead.
    pub(crate) fn push_work_if_looper(&self, work: Arc<dyn DeliverToRead>) -> BinderResult {
        let mut inner = self.inner.lock();
        if inner.is_looper() && !inner.is_dead {
            inner.push_work(work);
            Ok(())
        } else {
            drop(inner);
            self.process.push_work(work)
        }
    }

    pub(crate) fn push_work_deferred(&self, work: Arc<dyn DeliverToRead>) {
        self.inner.lock().push_work_deferred(work);
    }

    fn translate_object(
        &self,
        index_offset: usize,
        view: &mut AllocationView<'_, '_>,
        allow_fds: bool,
    ) -> BinderResult {
        let offset = view.alloc.read(index_offset)?;
        let header = view.read::<bindings::binder_object_header>(offset)?;
        // TODO: Handle other types.
        match header.type_ {
            BINDER_TYPE_WEAK_BINDER | BINDER_TYPE_BINDER => {
                let strong = header.type_ == BINDER_TYPE_BINDER;
                view.transfer_binder_object(offset, strong, |obj| {
                    // SAFETY: `binder` is a `binder_uintptr_t`; any bit pattern is a valid
                    // representation.
                    let ptr = unsafe { obj.__bindgen_anon_1.binder } as _;
                    let cookie = obj.cookie as _;
                    let flags = obj.flags as _;
                    let node = self.process.as_arc_borrow().get_node(
                        ptr,
                        cookie,
                        flags,
                        strong,
                        Some(self),
                    )?;
                    security::binder_transfer_binder(&self.process.cred, &view.alloc.process.cred)?;
                    Ok(node)
                })?;
            }
            BINDER_TYPE_WEAK_HANDLE | BINDER_TYPE_HANDLE => {
                let strong = header.type_ == BINDER_TYPE_HANDLE;
                view.transfer_binder_object(offset, strong, |obj| {
                    // SAFETY: `handle` is a `u32`; any bit pattern is a valid representation.
                    let handle = unsafe { obj.__bindgen_anon_1.handle } as _;
                    let node = self.process.get_node_from_handle(handle, strong)?;
                    security::binder_transfer_binder(&self.process.cred, &view.alloc.process.cred)?;
                    Ok(node)
                })?;
            }
            BINDER_TYPE_FD => {
                if !allow_fds {
                    return Err(BinderError::new_failed());
                }

                let obj = view.read::<bindings::binder_fd_object>(offset)?;
                // SAFETY: `fd` is a `u32`; any bit pattern is a valid representation.
                let fd = unsafe { obj.__bindgen_anon_1.fd };
                let file = File::from_fd(fd)?;
                security::binder_transfer_file(
                    &self.process.cred,
                    &view.alloc.process.cred,
                    &file,
                )?;
                let field_offset =
                    kernel::offset_of!(bindings::binder_fd_object, __bindgen_anon_1.fd) as usize;
                let file_info = Box::try_new(FileInfo::new(file, offset + field_offset))?;
                view.alloc.add_file_info(file_info);
            }
            _ => pr_warn!("Unsupported binder object type: {:x}\n", header.type_),
        }
        Ok(())
    }

    fn translate_objects(
        &self,
        alloc: &mut Allocation<'_>,
        start: usize,
        end: usize,
        allow_fds: bool,
    ) -> BinderResult {
        let mut view = AllocationView::new(alloc, start);
        for i in (start..end).step_by(size_of::<usize>()) {
            if let Err(err) = self.translate_object(i, &mut view, allow_fds) {
                alloc.set_info(AllocationInfo { offsets: start..i });
                return Err(err);
            }
        }
        alloc.set_info(AllocationInfo {
            offsets: start..end,
        });
        Ok(())
    }

    pub(crate) fn copy_transaction_data<'a>(
        &self,
        to_process: &'a Process,
        tr: &BinderTransactionData,
        allow_fds: bool,
    ) -> BinderResult<Allocation<'a>> {
        let data_size = tr.data_size as _;
        let adata_size = ptr_align(data_size);
        let offsets_size = tr.offsets_size as _;
        let aoffsets_size = ptr_align(offsets_size);

        // This guarantees that at least `sizeof(usize)` bytes will be allocated.
        let len = core::cmp::max(
            adata_size.checked_add(aoffsets_size).ok_or(ENOMEM)?,
            size_of::<usize>(),
        );
        let mut alloc = to_process.buffer_alloc(len)?;

        // Copy raw data.
        let mut reader = unsafe { UserSlicePtr::new(tr.data.ptr.buffer as _, data_size) }.reader();
        alloc.copy_into(&mut reader, 0, data_size)?;

        // Copy offsets if there are any.
        if offsets_size > 0 {
            let mut reader =
                unsafe { UserSlicePtr::new(tr.data.ptr.offsets as _, offsets_size) }.reader();
            alloc.copy_into(&mut reader, adata_size, offsets_size)?;

            // Traverse the objects specified.
            self.translate_objects(
                &mut alloc,
                adata_size,
                adata_size + aoffsets_size,
                allow_fds,
            )?;
        }

        Ok(alloc)
    }

    fn unwind_transaction_stack(self: &Arc<Self>) {
        let mut thread = self.clone();
        while let Ok(transaction) = {
            let mut inner = thread.inner.lock();
            inner.pop_transaction_to_reply(thread.as_ref())
        } {
            let reply = Either::Right(BR_DEAD_REPLY);
            if !transaction.from.deliver_single_reply(reply, &transaction) {
                break;
            }

            thread = transaction.from.clone();
        }
    }

    pub(crate) fn deliver_reply(
        &self,
        reply: Either<Arc<Transaction>, u32>,
        transaction: &Arc<Transaction>,
    ) {
        if self.deliver_single_reply(reply, transaction) {
            transaction.from.unwind_transaction_stack();
        }
    }

    /// Delivers a reply to the thread that started a transaction. The reply can either be a
    /// reply-transaction or an error code to be delivered instead.
    ///
    /// Returns whether the thread is dead. If it is, the caller is expected to unwind the
    /// transaction stack by completing transactions for threads that are dead.
    fn deliver_single_reply(
        &self,
        reply: Either<Arc<Transaction>, u32>,
        transaction: &Arc<Transaction>,
    ) -> bool {
        {
            let mut inner = self.inner.lock();
            if !inner.pop_transaction_replied(transaction) {
                return false;
            }

            if inner.is_dead {
                return true;
            }

            match reply {
                Either::Left(work) => inner.push_work(work),
                Either::Right(code) => inner.push_reply_work(code),
            }
        }

        // Notify the thread now that we've released the inner lock.
        self.work_condvar.notify_one();
        false
    }

    /// Determines if the given transaction is the current transaction for this thread.
    fn is_current_transaction(&self, transaction: &Arc<Transaction>) -> bool {
        let inner = self.inner.lock();
        match &inner.current_transaction {
            None => false,
            Some(current) => Arc::ptr_eq(current, transaction),
        }
    }

    fn transaction<T>(self: &Arc<Self>, tr: &BinderTransactionData, inner: T)
    where
        T: FnOnce(&Arc<Self>, &BinderTransactionData) -> BinderResult,
    {
        if let Err(err) = inner(self, tr) {
            self.inner.lock().push_return_work(err.reply);
        }
    }

    fn reply_inner(self: &Arc<Self>, tr: &BinderTransactionData) -> BinderResult {
        let orig = self.inner.lock().pop_transaction_to_reply(self)?;
        if !orig.from.is_current_transaction(&orig) {
            return Err(BinderError::new_failed());
        }

        // We need to complete the transaction even if we cannot complete building the reply.
        (|| -> BinderResult<_> {
            let completion = Arc::try_new(DeliverCode::new(BR_TRANSACTION_COMPLETE))?;
            let process = orig.from.process.clone();
            let allow_fds = orig.flags & TF_ACCEPT_FDS != 0;
            let reply = Transaction::new_reply(self, process, tr, allow_fds)?;
            self.inner.lock().push_work(completion);
            orig.from.deliver_reply(Either::Left(reply), &orig);
            Ok(())
        })()
        .map_err(|mut err| {
            // At this point we only return `BR_TRANSACTION_COMPLETE` to the caller, and we must let
            // the sender know that the transaction has completed (with an error in this case).
            let reply = Either::Right(BR_FAILED_REPLY);
            orig.from.deliver_reply(reply, &orig);
            err.reply = BR_TRANSACTION_COMPLETE;
            err
        })
    }

    /// Determines the current top of the transaction stack. It fails if the top is in another
    /// thread (i.e., this thread belongs to a stack but it has called another thread). The top is
    /// [`None`] if the thread is not currently participating in a transaction stack.
    fn top_of_transaction_stack(&self) -> Result<Option<Arc<Transaction>>> {
        let inner = self.inner.lock();
        Ok(if let Some(cur) = &inner.current_transaction {
            if core::ptr::eq(self, cur.from.as_ref()) {
                return Err(EINVAL);
            }
            Some(cur.clone())
        } else {
            None
        })
    }

    fn oneway_transaction_inner(self: &Arc<Self>, tr: &BinderTransactionData) -> BinderResult {
        let handle = unsafe { tr.target.handle };
        let node_ref = self.process.get_transaction_node(handle)?;
        security::binder_transaction(&self.process.cred, &node_ref.node.owner.cred)?;
        let completion = Arc::try_new(DeliverCode::new(BR_TRANSACTION_COMPLETE))?;
        let transaction = Transaction::new(node_ref, None, self, tr)?;
        self.inner.lock().push_work(completion);
        // TODO: Remove the completion on error?
        transaction.submit()?;
        Ok(())
    }

    fn transaction_inner(self: &Arc<Self>, tr: &BinderTransactionData) -> BinderResult {
        let handle = unsafe { tr.target.handle };
        let node_ref = self.process.get_transaction_node(handle)?;
        security::binder_transaction(&self.process.cred, &node_ref.node.owner.cred)?;
        // TODO: We need to ensure that there isn't a pending transaction in the work queue. How
        // could this happen?
        let top = self.top_of_transaction_stack()?;
        let completion = Arc::try_new(DeliverCode::new(BR_TRANSACTION_COMPLETE))?;
        let transaction = Transaction::new(node_ref, top, self, tr)?;

        // Check that the transaction stack hasn't changed while the lock was released, then update
        // it with the new transaction.
        {
            let mut inner = self.inner.lock();
            if !transaction.is_stacked_on(&inner.current_transaction) {
                return Err(BinderError::new_failed());
            }
            inner.current_transaction = Some(transaction.clone());
        }

        // We push the completion as a deferred work so that we wait for the reply before returning
        // to userland.
        self.push_work_deferred(completion);
        // TODO: Remove completion if submission fails?
        transaction.submit()?;
        Ok(())
    }

    fn write(self: &Arc<Self>, req: &mut BinderWriteRead) -> Result {
        let write_start = req.write_buffer.wrapping_add(req.write_consumed);
        let write_len = req.write_size - req.write_consumed;
        let mut reader = unsafe { UserSlicePtr::new(write_start as _, write_len as _).reader() };

        while reader.len() >= size_of::<u32>() && self.inner.lock().return_work.is_some() {
            let before = reader.len();
            match reader.read::<u32>()? {
                BC_TRANSACTION => {
                    let tr = reader.read::<BinderTransactionData>()?;
                    if tr.flags & TF_ONE_WAY != 0 {
                        self.transaction(&tr, Self::oneway_transaction_inner)
                    } else {
                        self.transaction(&tr, Self::transaction_inner)
                    }
                }
                BC_REPLY => self.transaction(&reader.read()?, Self::reply_inner),
                BC_FREE_BUFFER => drop(self.process.buffer_get(reader.read()?)),
                BC_INCREFS => self.process.update_ref(reader.read()?, true, false)?,
                BC_ACQUIRE => self.process.update_ref(reader.read()?, true, true)?,
                BC_RELEASE => self.process.update_ref(reader.read()?, false, true)?,
                BC_DECREFS => self.process.update_ref(reader.read()?, false, false)?,
                BC_INCREFS_DONE => self.process.inc_ref_done(&mut reader, false)?,
                BC_ACQUIRE_DONE => self.process.inc_ref_done(&mut reader, true)?,
                BC_REQUEST_DEATH_NOTIFICATION => self.process.request_death(&mut reader, self)?,
                BC_CLEAR_DEATH_NOTIFICATION => self.process.clear_death(&mut reader, self)?,
                BC_DEAD_BINDER_DONE => self.process.dead_binder_done(reader.read()?, self),
                BC_REGISTER_LOOPER => {
                    let valid = self.process.register_thread();
                    self.inner.lock().looper_register(valid);
                }
                BC_ENTER_LOOPER => self.inner.lock().looper_enter(),
                BC_EXIT_LOOPER => self.inner.lock().looper_exit(),

                // TODO: Add support for BC_TRANSACTION_SG and BC_REPLY_SG.
                // BC_ATTEMPT_ACQUIRE and BC_ACQUIRE_RESULT are no longer supported.
                _ => return Err(EINVAL),
            }

            // Update the number of write bytes consumed.
            req.write_consumed += (before - reader.len()) as u64;
        }
        Ok(())
    }

    fn read(self: &Arc<Self>, req: &mut BinderWriteRead, wait: bool) -> Result {
        let read_start = req.read_buffer.wrapping_add(req.read_consumed);
        let read_len = req.read_size - req.read_consumed;
        let mut writer = unsafe { UserSlicePtr::new(read_start as _, read_len as _) }.writer();
        let (in_pool, getter) = {
            let inner = self.inner.lock();
            (
                inner.is_looper(),
                if inner.should_use_process_work_queue() {
                    Self::get_work
                } else {
                    Self::get_work_local
                },
            )
        };

        // Reserve some room at the beginning of the read buffer so that we can send a
        // BR_SPAWN_LOOPER if we need to.
        if req.read_consumed == 0 {
            writer.write(&BR_NOOP)?;
        }

        // Loop doing work while there is room in the buffer.
        let initial_len = writer.len();
        while writer.len() >= size_of::<u32>() {
            match getter(self, wait && initial_len == writer.len()) {
                Ok(work) => {
                    if !work.do_work(self, &mut writer)? {
                        break;
                    }
                }
                Err(err) => {
                    // Propagate the error if we haven't written anything else.
                    if initial_len == writer.len() {
                        return Err(err);
                    } else {
                        break;
                    }
                }
            }
        }

        req.read_consumed += read_len - writer.len() as u64;

        // Write BR_SPAWN_LOOPER if the process needs more threads for its pool.
        if in_pool && self.process.needs_thread() {
            let mut writer =
                unsafe { UserSlicePtr::new(req.read_buffer as _, req.read_size as _) }.writer();
            writer.write(&BR_SPAWN_LOOPER)?;
        }

        Ok(())
    }

    pub(crate) fn write_read(self: &Arc<Self>, data: UserSlicePtr, wait: bool) -> Result {
        let (mut reader, mut writer) = data.reader_writer();
        let mut req = reader.read::<BinderWriteRead>()?;

        // TODO: `write(&req)` happens in all exit paths from here on. Find a better way to encode
        // it.

        // Go through the write buffer.
        if req.write_size > 0 {
            if let Err(err) = self.write(&mut req) {
                req.read_consumed = 0;
                writer.write(&req)?;
                return Err(err);
            }
        }

        // Go through the work queue.
        let mut ret = Ok(());
        if req.read_size > 0 {
            ret = self.read(&mut req, wait);
        }

        // Write the request back so that the consumed fields are visible to the caller.
        writer.write(&req)?;
        ret
    }

    pub(crate) fn poll(&self, file: &File, table: &PollTable) -> (bool, u32) {
        // SAFETY: `free_waiters` is called on release.
        unsafe { table.register_wait(file, &self.work_condvar) };
        let mut inner = self.inner.lock();
        (inner.should_use_process_work_queue(), inner.poll())
    }

    pub(crate) fn notify_if_poll_ready(&self) {
        // Determine if we need to notify. This requires the lock.
        let inner = self.inner.lock();
        let notify = inner.looper_flags & LOOPER_POLL != 0
            && inner.should_use_process_work_queue()
            && !inner.has_work();
        drop(inner);

        // Now that the lock is no longer held, notify the waiters if we have to.
        if notify {
            self.work_condvar.notify_one();
        }
    }

    pub(crate) fn push_return_work(&self, code: u32) {
        self.inner.lock().push_return_work(code)
    }

    pub(crate) fn release(self: &Arc<Self>) {
        // Mark the thread as dead.
        self.inner.lock().is_dead = true;

        // Cancel all pending work items.
        while let Ok(work) = self.get_work_local(false) {
            work.cancel();
        }

        // Complete the transaction stack as far as we can.
        self.unwind_transaction_stack();

        // Remove epoll items if polling was ever used on the thread.
        let poller = self.inner.lock().looper_flags & LOOPER_POLL != 0;
        if poller {
            self.work_condvar.free_waiters();

            unsafe { bindings::synchronize_rcu() };
        }
    }
}

impl GetLinks for Thread {
    type EntryType = Thread;
    fn get_links(data: &Thread) -> &Links<Thread> {
        &data.links
    }
}

struct ThreadError {
    error_code: AtomicU32,
    return_fn: fn(&mut InnerThread, Arc<ThreadError>),
    links: Links<dyn DeliverToRead>,
}

impl ThreadError {
    fn new(return_fn: fn(&mut InnerThread, Arc<ThreadError>)) -> Self {
        Self {
            error_code: AtomicU32::new(BR_OK),
            return_fn,
            links: Links::new(),
        }
    }
}

impl DeliverToRead for ThreadError {
    fn do_work(self: Arc<Self>, thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool> {
        // See `ThreadInner::push_existing_work` for the reason why `error_code` is up to date even
        // though we use relaxed semantics.
        let code = self.error_code.load(Ordering::Relaxed);

        // Return the `ThreadError` to the thread.
        #[allow(clippy::explicit_auto_deref)]
        (self.return_fn)(&mut *thread.inner.lock(), self);

        // Deliver the error code to userspace.
        writer.write(&code)?;
        Ok(true)
    }

    fn get_links(&self) -> &Links<dyn DeliverToRead> {
        &self.links
    }
}
