// SPDX-License-Identifier: GPL-2.0

use core::{convert::TryFrom, mem::take, ops::Range};
use kernel::{
    bindings,
    cred::Credential,
    file::{self, File, IoctlCommand, IoctlHandler, PollTable},
    io_buffer::{IoBufferReader, IoBufferWriter},
    linked_list::List,
    mm, new_mutex,
    pages::Pages,
    prelude::*,
    rbtree::RBTree,
    sync::{Arc, ArcBorrow, Guard, Mutex, UniqueArc},
    task::Task,
    user_ptr::{UserSlicePtr, UserSlicePtrReader},
    Either,
};

use crate::{
    allocation::Allocation,
    context::Context,
    defs::*,
    node::{Node, NodeDeath, NodeRef},
    range_alloc::RangeAllocator,
    thread::{BinderError, BinderResult, Thread},
    DeliverToRead, DeliverToReadListAdapter,
};

// TODO: Review this:
// Lock order: Process::node_refs -> Process::inner -> Thread::inner

pub(crate) struct AllocationInfo {
    /// Range within the allocation where we can find the offsets to the object descriptors.
    pub(crate) offsets: Range<usize>,
}

struct Mapping {
    address: usize,
    alloc: RangeAllocator<AllocationInfo>,
    pages: Arc<[Pages<0>]>,
}

impl Mapping {
    fn new(address: usize, size: usize, pages: Arc<[Pages<0>]>) -> Result<Self> {
        let alloc = RangeAllocator::new(size)?;
        Ok(Self {
            address,
            alloc,
            pages,
        })
    }
}

// TODO: Make this private.
pub(crate) struct ProcessInner {
    is_manager: bool,
    is_dead: bool,
    threads: RBTree<i32, Arc<Thread>>,
    ready_threads: List<Arc<Thread>>,
    work: List<DeliverToReadListAdapter>,
    mapping: Option<Mapping>,
    nodes: RBTree<usize, Arc<Node>>,

    delivered_deaths: List<Arc<NodeDeath>>,

    /// The number of requested threads that haven't registered yet.
    requested_thread_count: u32,

    /// The maximum number of threads used by the process thread pool.
    max_threads: u32,

    /// The number of threads the started and registered with the thread pool.
    started_thread_count: u32,
}

impl ProcessInner {
    fn new() -> Self {
        Self {
            is_manager: false,
            is_dead: false,
            threads: RBTree::new(),
            ready_threads: List::new(),
            work: List::new(),
            mapping: None,
            nodes: RBTree::new(),
            requested_thread_count: 0,
            max_threads: 0,
            started_thread_count: 0,
            delivered_deaths: List::new(),
        }
    }

    fn push_work(&mut self, work: Arc<dyn DeliverToRead>) -> BinderResult {
        // Try to find a ready thread to which to push the work.
        if let Some(thread) = self.ready_threads.pop_front() {
            // Push to thread while holding state lock. This prevents the thread from giving up
            // (for example, because of a signal) when we're about to deliver work.
            thread.push_work(work)
        } else if self.is_dead {
            Err(BinderError::new_dead())
        } else {
            // There are no ready threads. Push work to process queue.
            self.work.push_back(work);

            // Wake up polling threads, if any.
            for thread in self.threads.values() {
                thread.notify_if_poll_ready();
            }
            Ok(())
        }
    }

    // TODO: Should this be private?
    pub(crate) fn remove_node(&mut self, ptr: usize) {
        self.nodes.remove(&ptr);
    }

    /// Updates the reference count on the given node.
    // TODO: Decide if this should be private.
    pub(crate) fn update_node_refcount(
        &mut self,
        node: &Arc<Node>,
        inc: bool,
        strong: bool,
        biased: bool,
        othread: Option<&Thread>,
    ) {
        let push = node.update_refcount_locked(inc, strong, biased, self);

        // If we decided that we need to push work, push either to the process or to a thread if
        // one is specified.
        if push {
            if let Some(thread) = othread {
                thread.push_work_deferred(node.clone());
            } else {
                let _ = self.push_work(node.clone());
                // Nothing to do: `push_work` may fail if the process is dead, but that's ok as in
                // that case, it doesn't care about the notification.
            }
        }
    }

    // TODO: Make this private.
    pub(crate) fn new_node_ref(
        &mut self,
        node: Arc<Node>,
        strong: bool,
        thread: Option<&Thread>,
    ) -> NodeRef {
        self.update_node_refcount(&node, true, strong, false, thread);
        #[allow(clippy::bool_to_int_with_if)]
        let strong_count = if strong { 1 } else { 0 };
        NodeRef::new(node, strong_count, 1 - strong_count)
    }

    /// Returns an existing node with the given pointer and cookie, if one exists.
    ///
    /// Returns an error if a node with the given pointer but a different cookie exists.
    fn get_existing_node(&self, ptr: usize, cookie: usize) -> Result<Option<Arc<Node>>> {
        match self.nodes.get(&ptr) {
            None => Ok(None),
            Some(node) => {
                let (_, node_cookie) = node.get_id();
                if node_cookie == cookie {
                    Ok(Some(node.clone()))
                } else {
                    Err(EINVAL)
                }
            }
        }
    }

    /// Returns a reference to an existing node with the given pointer and cookie. It requires a
    /// mutable reference because it needs to increment the ref count on the node, which may
    /// require pushing work to the work queue (to notify userspace of 0 to 1 transitions).
    fn get_existing_node_ref(
        &mut self,
        ptr: usize,
        cookie: usize,
        strong: bool,
        thread: Option<&Thread>,
    ) -> Result<Option<NodeRef>> {
        Ok(self
            .get_existing_node(ptr, cookie)?
            .map(|node| self.new_node_ref(node, strong, thread)))
    }

    fn register_thread(&mut self) -> bool {
        if self.requested_thread_count == 0 {
            return false;
        }

        self.requested_thread_count -= 1;
        self.started_thread_count += 1;
        true
    }

    /// Finds a delivered death notification with the given cookie, removes it from the thread's
    /// delivered list, and returns it.
    fn pull_delivered_death(&mut self, cookie: usize) -> Option<Arc<NodeDeath>> {
        let mut cursor = self.delivered_deaths.cursor_front_mut();
        while let Some(death) = cursor.current() {
            if death.cookie == cookie {
                return cursor.remove_current();
            }
            cursor.move_next();
        }
        None
    }

    pub(crate) fn death_delivered(&mut self, death: Arc<NodeDeath>) {
        self.delivered_deaths.push_back(death);
    }
}

struct NodeRefInfo {
    node_ref: NodeRef,
    death: Option<Arc<NodeDeath>>,
}

impl NodeRefInfo {
    fn new(node_ref: NodeRef) -> Self {
        Self {
            node_ref,
            death: None,
        }
    }
}

struct ProcessNodeRefs {
    by_handle: RBTree<u32, NodeRefInfo>,
    by_global_id: RBTree<u64, u32>,
}

impl ProcessNodeRefs {
    fn new() -> Self {
        Self {
            by_handle: RBTree::new(),
            by_global_id: RBTree::new(),
        }
    }
}

#[pin_project]
pub(crate) struct Process {
    ctx: Arc<Context>,

    // The task leader (process).
    pub(crate) task: ARef<Task>,

    // Credential associated with file when `Process` is created.
    pub(crate) cred: ARef<Credential>,

    // TODO: For now this a mutex because we have allocations in RangeAllocator while holding the
    // lock. We may want to split up the process state at some point to use a spin lock for the
    // other fields.
    // TODO: Make this private again.
    #[pin]
    pub(crate) inner: Mutex<ProcessInner>,

    // References are in a different mutex to avoid recursive acquisition when
    // incrementing/decrementing a node in another process.
    #[pin]
    node_refs: Mutex<ProcessNodeRefs>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl Process {
    fn new(ctx: Arc<Context>, cred: ARef<Credential>) -> Result<Arc<Self>> {
        Arc::pin_init(pin_init!(Self {
            ctx,
            cred,
            task: ARef::from(Task::current().group_leader()),
            inner: new_mutex!(ProcessInner::new(), "Process::inner"),
            node_refs: new_mutex!(ProcessNodeRefs::new(), "Process::node_refs"),
        }))
    }

    /// Attempts to fetch a work item from the process queue.
    pub(crate) fn get_work(&self) -> Option<Arc<dyn DeliverToRead>> {
        self.inner.lock().work.pop_front()
    }

    /// Attempts to fetch a work item from the process queue. If none is available, it registers the
    /// given thread as ready to receive work directly.
    ///
    /// This must only be called when the thread is not participating in a transaction chain; when
    /// it is, work will always be delivered directly to the thread (and not through the process
    /// queue).
    pub(crate) fn get_work_or_register<'a>(
        &'a self,
        thread: &'a Arc<Thread>,
    ) -> Either<Arc<dyn DeliverToRead>, Registration<'a>> {
        let mut inner = self.inner.lock();

        // Try to get work from the process queue.
        if let Some(work) = inner.work.pop_front() {
            return Either::Left(work);
        }

        // Register the thread as ready.
        Either::Right(Registration::new(self, thread, &mut inner))
    }

    fn get_thread(self: ArcBorrow<'_, Self>, id: i32) -> Result<Arc<Thread>> {
        // TODO: Consider using read/write locks here instead.
        {
            let inner = self.inner.lock();
            if let Some(thread) = inner.threads.get(&id) {
                return Ok(thread.clone());
            }
        }

        // Allocate a new `Thread` without holding any locks.
        let ta = Thread::new(id, self.into())?;
        let node = RBTree::try_allocate_node(id, ta.clone())?;

        let mut inner = self.inner.lock();

        // Recheck. It's possible the thread was create while we were not holding the lock.
        if let Some(thread) = inner.threads.get(&id) {
            return Ok(thread.clone());
        }

        inner.threads.insert(node);
        Ok(ta)
    }

    pub(crate) fn push_work(&self, work: Arc<dyn DeliverToRead>) -> BinderResult {
        self.inner.lock().push_work(work)
    }

    fn set_as_manager(
        self: ArcBorrow<'_, Self>,
        info: Option<FlatBinderObject>,
        thread: &Thread,
    ) -> Result {
        let (ptr, cookie, flags) = if let Some(obj) = info {
            (
                // SAFETY: The object type for this ioctl is implicitly `BINDER_TYPE_BINDER`, so it
                // is safe to access the `binder` field.
                unsafe { obj.__bindgen_anon_1.binder },
                obj.cookie,
                obj.flags,
            )
        } else {
            (0, 0, 0)
        };
        let node_ref = self.get_node(ptr as _, cookie as _, flags as _, true, Some(thread))?;
        let node = node_ref.node.clone();
        self.ctx.set_manager_node(node_ref)?;
        self.inner.lock().is_manager = true;

        // Force the state of the node to prevent the delivery of acquire/increfs.
        let mut owner_inner = node.owner.inner.lock();
        node.force_has_count(&mut owner_inner);
        Ok(())
    }

    pub(crate) fn get_node(
        self: ArcBorrow<'_, Self>,
        ptr: usize,
        cookie: usize,
        flags: u32,
        strong: bool,
        thread: Option<&Thread>,
    ) -> Result<NodeRef> {
        // Try to find an existing node.
        {
            let mut inner = self.inner.lock();
            if let Some(node) = inner.get_existing_node_ref(ptr, cookie, strong, thread)? {
                return Ok(node);
            }
        }

        // Allocate the node before reacquiring the lock.
        let node = Arc::try_new(Node::new(ptr, cookie, flags, self.into()))?;
        let rbnode = RBTree::try_allocate_node(ptr, node.clone())?;

        let mut inner = self.inner.lock();
        if let Some(node) = inner.get_existing_node_ref(ptr, cookie, strong, thread)? {
            return Ok(node);
        }

        inner.nodes.insert(rbnode);
        Ok(inner.new_node_ref(node, strong, thread))
    }

    pub(crate) fn insert_or_update_handle(
        &self,
        node_ref: NodeRef,
        is_mananger: bool,
    ) -> Result<u32> {
        {
            let mut refs = self.node_refs.lock();

            // Do a lookup before inserting.
            if let Some(handle_ref) = refs.by_global_id.get(&node_ref.node.global_id) {
                let handle = *handle_ref;
                let info = refs.by_handle.get_mut(&handle).unwrap();
                info.node_ref.absorb(node_ref);
                return Ok(handle);
            }
        }

        // Reserve memory for tree nodes.
        let reserve1 = RBTree::try_reserve_node()?;
        let reserve2 = RBTree::try_reserve_node()?;

        let mut refs = self.node_refs.lock();

        // Do a lookup again as node may have been inserted before the lock was reacquired.
        if let Some(handle_ref) = refs.by_global_id.get(&node_ref.node.global_id) {
            let handle = *handle_ref;
            let info = refs.by_handle.get_mut(&handle).unwrap();
            info.node_ref.absorb(node_ref);
            return Ok(handle);
        }

        // Find id.
        #[allow(clippy::bool_to_int_with_if)]
        let mut target = if is_mananger { 0 } else { 1 };
        for handle in refs.by_handle.keys() {
            if *handle > target {
                break;
            }
            if *handle == target {
                target = target.checked_add(1).ok_or(ENOMEM)?;
            }
        }

        // Ensure the process is still alive while we insert a new reference.
        let inner = self.inner.lock();
        if inner.is_dead {
            return Err(ESRCH);
        }
        refs.by_global_id
            .insert(reserve1.into_node(node_ref.node.global_id, target));
        refs.by_handle
            .insert(reserve2.into_node(target, NodeRefInfo::new(node_ref)));
        Ok(target)
    }

    pub(crate) fn get_transaction_node(&self, handle: u32) -> BinderResult<NodeRef> {
        // When handle is zero, try to get the context manager.
        if handle == 0 {
            self.ctx.get_manager_node(true)
        } else {
            self.get_node_from_handle(handle, true)
        }
    }

    pub(crate) fn get_node_from_handle(&self, handle: u32, strong: bool) -> BinderResult<NodeRef> {
        self.node_refs
            .lock()
            .by_handle
            .get(&handle)
            .ok_or(ENOENT)?
            .node_ref
            .clone(strong)
    }

    pub(crate) fn remove_from_delivered_deaths(&self, death: &Arc<NodeDeath>) {
        let mut inner = self.inner.lock();
        let removed = unsafe { inner.delivered_deaths.remove(death) };
        drop(inner);
        drop(removed);
    }

    pub(crate) fn update_ref(&self, handle: u32, inc: bool, strong: bool) -> Result {
        if inc && handle == 0 {
            if let Ok(node_ref) = self.ctx.get_manager_node(strong) {
                if core::ptr::eq(self, &*node_ref.node.owner) {
                    return Err(EINVAL);
                }
                let _ = self.insert_or_update_handle(node_ref, true);
                return Ok(());
            }
        }

        // To preserve original binder behaviour, we only fail requests where the manager tries to
        // increment references on itself.
        let mut refs = self.node_refs.lock();
        if let Some(info) = refs.by_handle.get_mut(&handle) {
            if info.node_ref.update(inc, strong) {
                // Clean up death if there is one attached to this node reference.
                if let Some(death) = info.death.take() {
                    death.set_cleared(true);
                    self.remove_from_delivered_deaths(&death);
                }

                // Remove reference from process tables.
                let id = info.node_ref.node.global_id;
                refs.by_handle.remove(&handle);
                refs.by_global_id.remove(&id);
            }
        }
        Ok(())
    }

    /// Decrements the refcount of the given node, if one exists.
    pub(crate) fn update_node(&self, ptr: usize, cookie: usize, strong: bool, biased: bool) {
        let mut inner = self.inner.lock();
        if let Ok(Some(node)) = inner.get_existing_node(ptr, cookie) {
            inner.update_node_refcount(&node, false, strong, biased, None);
        }
    }

    pub(crate) fn inc_ref_done(&self, reader: &mut UserSlicePtrReader, strong: bool) -> Result {
        let ptr = reader.read::<usize>()?;
        let cookie = reader.read::<usize>()?;
        self.update_node(ptr, cookie, strong, true);
        Ok(())
    }

    pub(crate) fn buffer_alloc(&self, size: usize) -> BinderResult<Allocation<'_>> {
        let mut inner = self.inner.lock();
        let mapping = inner.mapping.as_mut().ok_or_else(BinderError::new_dead)?;

        let offset = mapping.alloc.reserve_new(size)?;
        Ok(Allocation::new(
            self,
            offset,
            size,
            mapping.address + offset,
            mapping.pages.clone(),
        ))
    }

    // TODO: Review if we want an Option or a Result.
    pub(crate) fn buffer_get(&self, ptr: usize) -> Option<Allocation<'_>> {
        let mut inner = self.inner.lock();
        let mapping = inner.mapping.as_mut()?;
        let offset = ptr.checked_sub(mapping.address)?;
        let (size, odata) = mapping.alloc.reserve_existing(offset).ok()?;
        let mut alloc = Allocation::new(self, offset, size, ptr, mapping.pages.clone());
        if let Some(data) = odata {
            alloc.set_info(data);
        }
        Some(alloc)
    }

    pub(crate) fn buffer_raw_free(&self, ptr: usize) {
        let mut inner = self.inner.lock();
        if let Some(ref mut mapping) = &mut inner.mapping {
            if ptr < mapping.address
                || mapping
                    .alloc
                    .reservation_abort(ptr - mapping.address)
                    .is_err()
            {
                pr_warn!(
                    "Pointer {:x} failed to free, base = {:x}\n",
                    ptr,
                    mapping.address
                );
            }
        }
    }

    pub(crate) fn buffer_make_freeable(&self, offset: usize, data: Option<AllocationInfo>) {
        let mut inner = self.inner.lock();
        if let Some(ref mut mapping) = &mut inner.mapping {
            if mapping.alloc.reservation_commit(offset, data).is_err() {
                pr_warn!("Offset {} failed to be marked freeable\n", offset);
            }
        }
    }

    fn create_mapping(&self, vma: &mut mm::virt::Area) -> Result {
        let size = core::cmp::min(vma.end() - vma.start(), bindings::SZ_4M as usize);
        let page_count = size / kernel::PAGE_SIZE;

        // Allocate and map all pages.
        //
        // N.B. If we fail halfway through mapping these pages, the kernel will unmap them.
        let mut pages = Vec::new();
        pages.try_reserve_exact(page_count)?;
        let mut address = vma.start();
        for _ in 0..page_count {
            let page = Pages::<0>::new()?;
            vma.insert_page(address, &page)?;
            pages.try_push(page)?;
            address += kernel::PAGE_SIZE;
        }

        let ref_pages = Arc::try_from(pages)?;

        // Save pages for later.
        let mut inner = self.inner.lock();
        match &inner.mapping {
            None => inner.mapping = Some(Mapping::new(vma.start(), size, ref_pages)?),
            Some(_) => return Err(EBUSY),
        }
        Ok(())
    }

    fn version(&self, data: UserSlicePtr) -> Result {
        data.writer().write(&BinderVersion::current())
    }

    pub(crate) fn register_thread(&self) -> bool {
        self.inner.lock().register_thread()
    }

    fn remove_thread(&self, thread: Arc<Thread>) {
        self.inner.lock().threads.remove(&thread.id);
        thread.release();
    }

    fn set_max_threads(&self, max: u32) {
        self.inner.lock().max_threads = max;
    }

    fn get_node_debug_info(&self, data: UserSlicePtr) -> Result {
        let (mut reader, mut writer) = data.reader_writer();

        // Read the starting point.
        let ptr = reader.read::<BinderNodeDebugInfo>()?.ptr as usize;
        let mut out = BinderNodeDebugInfo::default();

        {
            let inner = self.inner.lock();
            for (node_ptr, node) in &inner.nodes {
                if *node_ptr > ptr {
                    node.populate_debug_info(&mut out, &inner);
                    break;
                }
            }
        }

        writer.write(&out)
    }

    fn get_node_info_from_ref(&self, data: UserSlicePtr) -> Result {
        let (mut reader, mut writer) = data.reader_writer();
        let mut out = reader.read::<BinderNodeInfoForRef>()?;

        if out.strong_count != 0
            || out.weak_count != 0
            || out.reserved1 != 0
            || out.reserved2 != 0
            || out.reserved3 != 0
        {
            return Err(EINVAL);
        }

        // Only the context manager is allowed to use this ioctl.
        if !self.inner.lock().is_manager {
            return Err(EPERM);
        }

        let node_ref = self
            .get_node_from_handle(out.handle, true)
            .or(Err(EINVAL))?;

        // Get the counts from the node.
        {
            let owner_inner = node_ref.node.owner.inner.lock();
            node_ref.node.populate_counts(&mut out, &owner_inner);
        }

        // Write the result back.
        writer.write(&out)
    }

    pub(crate) fn needs_thread(&self) -> bool {
        let mut inner = self.inner.lock();
        let ret = inner.requested_thread_count == 0
            && inner.ready_threads.is_empty()
            && inner.started_thread_count < inner.max_threads;
        if ret {
            inner.requested_thread_count += 1
        };
        ret
    }

    pub(crate) fn request_death(
        self: &Arc<Self>,
        reader: &mut UserSlicePtrReader,
        thread: &Thread,
    ) -> Result {
        let handle: u32 = reader.read()?;
        let cookie: usize = reader.read()?;

        // TODO: First two should result in error, but not the others.

        // TODO: Do we care about the context manager dying?

        // Queue BR_ERROR if we can't allocate memory for the death notification.
        let death = UniqueArc::try_new_uninit().map_err(|err| {
            thread.push_return_work(BR_ERROR);
            err
        })?;

        let mut refs = self.node_refs.lock();
        let info = refs.by_handle.get_mut(&handle).ok_or(EINVAL)?;

        // Nothing to do if there is already a death notification request for this handle.
        if info.death.is_some() {
            return Ok(());
        }

        let death = Arc::<NodeDeath>::from(
            death
                .pin_init_now::<core::convert::Infallible>(NodeDeath::new(
                    info.node_ref.node.clone(),
                    self.clone(),
                    cookie,
                ))
                .unwrap_or_else(|(err, _)| match err {}),
        );

        info.death = Some(death.clone());

        // Register the death notification.
        {
            let mut owner_inner = info.node_ref.node.owner.inner.lock();
            if owner_inner.is_dead {
                drop(owner_inner);
                let _ = self.push_work(death);
            } else {
                info.node_ref.node.add_death(death, &mut owner_inner);
            }
        }
        Ok(())
    }

    pub(crate) fn clear_death(&self, reader: &mut UserSlicePtrReader, thread: &Thread) -> Result {
        let handle: u32 = reader.read()?;
        let cookie: usize = reader.read()?;

        let mut refs = self.node_refs.lock();
        let info = refs.by_handle.get_mut(&handle).ok_or(EINVAL)?;

        let death = info.death.take().ok_or(EINVAL)?;
        if death.cookie != cookie {
            info.death = Some(death);
            return Err(EINVAL);
        }

        // Update state and determine if we need to queue a work item. We only need to do it when
        // the node is not dead or if the user already completed the death notification.
        if death.set_cleared(false) {
            let _ = thread.push_work_if_looper(death);
        }

        Ok(())
    }

    pub(crate) fn dead_binder_done(&self, cookie: usize, thread: &Thread) {
        if let Some(death) = self.inner.lock().pull_delivered_death(cookie) {
            death.set_notification_done(thread);
        }
    }
}

impl IoctlHandler for Process {
    type Target<'a> = ArcBorrow<'a, Process>;

    fn write(
        this: ArcBorrow<'_, Process>,
        _file: &File,
        cmd: u32,
        reader: &mut UserSlicePtrReader,
    ) -> Result<i32> {
        let thread = this.get_thread(Task::current().pid())?;
        match cmd {
            bindings::BINDER_SET_MAX_THREADS => this.set_max_threads(reader.read()?),
            bindings::BINDER_SET_CONTEXT_MGR => this.set_as_manager(None, &thread)?,
            bindings::BINDER_THREAD_EXIT => this.remove_thread(thread),
            bindings::BINDER_SET_CONTEXT_MGR_EXT => {
                this.set_as_manager(Some(reader.read()?), &thread)?
            }
            _ => return Err(EINVAL),
        }
        Ok(0)
    }

    fn read_write(
        this: ArcBorrow<'_, Process>,
        file: &File,
        cmd: u32,
        data: UserSlicePtr,
    ) -> Result<i32> {
        let thread = this.get_thread(Task::current().pid())?;
        let blocking = (file.flags() & file::flags::O_NONBLOCK) == 0;
        match cmd {
            bindings::BINDER_WRITE_READ => thread.write_read(data, blocking)?,
            bindings::BINDER_GET_NODE_DEBUG_INFO => this.get_node_debug_info(data)?,
            bindings::BINDER_GET_NODE_INFO_FOR_REF => this.get_node_info_from_ref(data)?,
            bindings::BINDER_VERSION => this.version(data)?,
            _ => return Err(EINVAL),
        }
        Ok(0)
    }
}

#[vtable]
impl file::Operations for Process {
    type Data = Arc<Self>;
    type OpenData = Arc<Context>;

    fn open(ctx: &Arc<Context>, file: &File) -> Result<Self::Data> {
        Self::new(ctx.clone(), file.cred().into())
    }

    fn release(obj: Self::Data, _file: &File) {
        // Mark this process as dead. We'll do the same for the threads later.
        obj.inner.lock().is_dead = true;

        // If this process is the manager, unset it.
        if obj.inner.lock().is_manager {
            obj.ctx.unset_manager_node();
        }

        // TODO: Do this in a worker?

        // Cancel all pending work items.
        while let Some(work) = obj.get_work() {
            work.cancel();
        }

        // Free any resources kept alive by allocated buffers.
        let omapping = obj.inner.lock().mapping.take();
        if let Some(mut mapping) = omapping {
            let address = mapping.address;
            let pages = mapping.pages.clone();
            mapping.alloc.for_each(|offset, size, odata| {
                let ptr = offset + address;
                let mut alloc = Allocation::new(&obj, offset, size, ptr, pages.clone());
                if let Some(data) = odata {
                    alloc.set_info(data);
                }
                drop(alloc)
            });
        }

        // Drop all references. We do this dance with `swap` to avoid destroying the references
        // while holding the lock.
        let mut refs = obj.node_refs.lock();
        let mut node_refs = take(&mut refs.by_handle);
        drop(refs);

        // Remove all death notifications from the nodes (that belong to a different process).
        for info in node_refs.values_mut() {
            let death = if let Some(existing) = info.death.take() {
                existing
            } else {
                continue;
            };

            death.set_cleared(false);
        }

        // Do similar dance for the state lock.
        let mut inner = obj.inner.lock();
        let threads = take(&mut inner.threads);
        let nodes = take(&mut inner.nodes);
        drop(inner);

        // Release all threads.
        for thread in threads.values() {
            thread.release();
        }

        // Deliver death notifications.
        for node in nodes.values() {
            loop {
                let death = {
                    let mut inner = obj.inner.lock();
                    if let Some(death) = node.next_death(&mut inner) {
                        death
                    } else {
                        break;
                    }
                };

                death.set_dead();
            }
        }
    }

    fn ioctl(this: ArcBorrow<'_, Process>, file: &File, cmd: &mut IoctlCommand) -> Result<i32> {
        cmd.dispatch::<Self>(this, file)
    }

    fn compat_ioctl(
        this: ArcBorrow<'_, Process>,
        file: &File,
        cmd: &mut IoctlCommand,
    ) -> Result<i32> {
        cmd.dispatch::<Self>(this, file)
    }

    fn mmap(this: ArcBorrow<'_, Process>, _file: &File, vma: &mut mm::virt::Area) -> Result {
        // We don't allow mmap to be used in a different process.
        if !core::ptr::eq(Task::current().group_leader(), &*this.task) {
            return Err(EINVAL);
        }

        if vma.start() == 0 {
            return Err(EINVAL);
        }

        let mut flags = vma.flags();
        use mm::virt::flags::*;
        if flags & WRITE != 0 {
            return Err(EPERM);
        }

        flags |= DONTCOPY | MIXEDMAP;
        flags &= !MAYWRITE;
        vma.set_flags(flags);

        // TODO: Set ops. We need to learn when the user unmaps so that we can stop using it.
        this.create_mapping(vma)
    }

    fn poll(this: ArcBorrow<'_, Process>, file: &File, table: &PollTable) -> Result<u32> {
        let thread = this.get_thread(Task::current().pid())?;
        let (from_proc, mut mask) = thread.poll(file, table);
        if mask == 0 && from_proc && !this.inner.lock().work.is_empty() {
            mask |= bindings::POLLIN;
        }
        Ok(mask)
    }
}

pub(crate) struct Registration<'a> {
    process: &'a Process,
    thread: &'a Arc<Thread>,
}

impl<'a> Registration<'a> {
    fn new(
        process: &'a Process,
        thread: &'a Arc<Thread>,
        guard: &mut Guard<'_, Mutex<ProcessInner>>,
    ) -> Self {
        guard.ready_threads.push_back(thread.clone());
        Self { process, thread }
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let mut inner = self.process.inner.lock();
        unsafe { inner.ready_threads.remove(self.thread) };
    }
}
