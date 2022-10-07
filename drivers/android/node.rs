// SPDX-License-Identifier: GPL-2.0

use core::sync::atomic::{AtomicU64, Ordering};
use kernel::{
    init::PinInit,
    io_buffer::IoBufferWriter,
    linked_list::{GetLinks, Links, List},
    new_spinlock,
    prelude::*,
    sync::{Arc, Guard, LockedBy, Mutex, SpinLock},
    user_ptr::UserSlicePtrWriter,
};

use crate::{
    defs::*,
    process::{Process, ProcessInner},
    thread::{BinderError, BinderResult, Thread},
    DeliverToRead,
};

struct CountState {
    count: usize,
    has_count: bool,
    is_biased: bool,
}

impl CountState {
    fn new() -> Self {
        Self {
            count: 0,
            has_count: false,
            is_biased: false,
        }
    }

    fn add_bias(&mut self) {
        self.count += 1;
        self.is_biased = true;
    }
}

struct NodeInner {
    strong: CountState,
    weak: CountState,
    death_list: List<Arc<NodeDeath>>,
}

struct NodeDeathInner {
    dead: bool,
    cleared: bool,
    notification_done: bool,

    /// Indicates whether the normal flow was interrupted by removing the handle. In this case, we
    /// need behave as if the death notification didn't exist (i.e., we don't deliver anything to
    /// the user.
    aborted: bool,
}

#[pin_project]
pub(crate) struct NodeDeath {
    node: Arc<Node>,
    process: Arc<Process>,
    // TODO: Make this private.
    pub(crate) cookie: usize,
    work_links: Links<dyn DeliverToRead>,
    // TODO: Add the moment we're using this for two lists, which isn't safe because we want to
    // remove from the list without knowing the list it's in. We need to separate this out.
    death_links: Links<NodeDeath>,
    #[pin]
    inner: SpinLock<NodeDeathInner>,
}

impl NodeDeath {
    /// Constructs a new node death notification object.
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new(node: Arc<Node>, process: Arc<Process>, cookie: usize) -> impl PinInit<Self> {
        pin_init!(Self {
            node,
            process,
            cookie,
            work_links: Links::new(),
            death_links: Links::new(),
            inner: new_spinlock!(
                NodeDeathInner {
                    dead: false,
                    cleared: false,
                    notification_done: false,
                    aborted: false,
                },
                "NodeDeath::inner"
            ),
        })
    }

    /// Sets the cleared flag to `true`.
    ///
    /// It removes `self` from the node's death notification list if needed. It must only be called
    /// once.
    ///
    /// Returns whether it needs to be queued.
    pub(crate) fn set_cleared(self: &Arc<Self>, abort: bool) -> bool {
        let (needs_removal, needs_queueing) = {
            // Update state and determine if we need to queue a work item. We only need to do it
            // when the node is not dead or if the user already completed the death notification.
            let mut inner = self.inner.lock();
            inner.cleared = true;
            if abort {
                inner.aborted = true;
            }
            (!inner.dead, !inner.dead || inner.notification_done)
        };

        // Remove death notification from node.
        if needs_removal {
            let mut owner_inner = self.node.owner.inner.lock();
            let node_inner = self.node.inner.access_mut(&mut owner_inner);
            unsafe { node_inner.death_list.remove(self) };
        }

        needs_queueing
    }

    /// Sets the 'notification done' flag to `true`.
    ///
    /// Returns whether it needs to be queued.
    pub(crate) fn set_notification_done(self: Arc<Self>, thread: &Thread) {
        let needs_queueing = {
            let mut inner = self.inner.lock();
            inner.notification_done = true;
            inner.cleared
        };

        if needs_queueing {
            let _ = thread.push_work_if_looper(self);
        }
    }

    /// Sets the 'dead' flag to `true` and queues work item if needed.
    pub(crate) fn set_dead(self: Arc<Self>) {
        let needs_queueing = {
            let mut inner = self.inner.lock();
            if inner.cleared {
                false
            } else {
                inner.dead = true;
                true
            }
        };

        if needs_queueing {
            // Push the death notification to the target process. There is nothing else to do if
            // it's already dead.
            let process = self.process.clone();
            let _ = process.push_work(self);
        }
    }
}

impl GetLinks for NodeDeath {
    type EntryType = NodeDeath;
    fn get_links(data: &NodeDeath) -> &Links<NodeDeath> {
        &data.death_links
    }
}

impl DeliverToRead for NodeDeath {
    fn do_work(self: Arc<Self>, _thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool> {
        let done = {
            let inner = self.inner.lock();
            if inner.aborted {
                return Ok(true);
            }
            inner.cleared && (!inner.dead || inner.notification_done)
        };

        let cookie = self.cookie;
        let cmd = if done {
            BR_CLEAR_DEATH_NOTIFICATION_DONE
        } else {
            let process = self.process.clone();
            let mut process_inner = process.inner.lock();
            let inner = self.inner.lock();
            if inner.aborted {
                return Ok(true);
            }
            // We're still holding the inner lock, so it cannot be aborted while we insert it into
            // the delivered list.
            process_inner.death_delivered(self.clone());
            BR_DEAD_BINDER
        };

        writer.write(&cmd)?;
        writer.write(&cookie)?;

        // Mimic the original code: we stop processing work items when we get to a death
        // notification.
        Ok(cmd != BR_DEAD_BINDER)
    }

    fn get_links(&self) -> &Links<dyn DeliverToRead> {
        &self.work_links
    }
}

pub(crate) struct Node {
    pub(crate) global_id: u64,
    ptr: usize,
    cookie: usize,
    pub(crate) flags: u32,
    pub(crate) owner: Arc<Process>,
    inner: LockedBy<NodeInner, Mutex<ProcessInner>>,
    links: Links<dyn DeliverToRead>,
}

impl Node {
    pub(crate) fn new(ptr: usize, cookie: usize, flags: u32, owner: Arc<Process>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let inner = LockedBy::new(
            &owner.inner,
            NodeInner {
                strong: CountState::new(),
                weak: CountState::new(),
                death_list: List::new(),
            },
        );
        Self {
            global_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ptr,
            cookie,
            flags,
            owner,
            inner,
            links: Links::new(),
        }
    }

    pub(crate) fn get_id(&self) -> (usize, usize) {
        (self.ptr, self.cookie)
    }

    pub(crate) fn next_death(
        &self,
        guard: &mut Guard<'_, Mutex<ProcessInner>>,
    ) -> Option<Arc<NodeDeath>> {
        self.inner.access_mut(guard).death_list.pop_front()
    }

    pub(crate) fn add_death(
        &self,
        death: Arc<NodeDeath>,
        guard: &mut Guard<'_, Mutex<ProcessInner>>,
    ) {
        self.inner.access_mut(guard).death_list.push_back(death);
    }

    pub(crate) fn update_refcount_locked(
        &self,
        inc: bool,
        strong: bool,
        biased: bool,
        owner_inner: &mut ProcessInner,
    ) -> bool {
        let inner = self.inner.access_from_mut(owner_inner);

        // Get a reference to the state we'll update.
        let state = if strong {
            &mut inner.strong
        } else {
            &mut inner.weak
        };

        // Update biased state: if the count is not biased, there is nothing to do; otherwise,
        // we're removing the bias, so mark the state as such.
        if biased {
            if !state.is_biased {
                return false;
            }

            state.is_biased = false;
        }

        // Update the count and determine whether we need to push work.
        // TODO: Here we may want to check the weak count being zero but the strong count being 1,
        // because in such cases, we won't deliver anything to userspace, so we shouldn't queue
        // either.
        if inc {
            state.count += 1;
            !state.has_count
        } else {
            state.count -= 1;
            state.count == 0 && state.has_count
        }
    }

    pub(crate) fn update_refcount(self: &Arc<Self>, inc: bool, strong: bool) {
        self.owner
            .inner
            .lock()
            .update_node_refcount(self, inc, strong, false, None);
    }

    pub(crate) fn populate_counts(
        &self,
        out: &mut BinderNodeInfoForRef,
        guard: &Guard<'_, Mutex<ProcessInner>>,
    ) {
        let inner = self.inner.access(guard);
        out.strong_count = inner.strong.count as _;
        out.weak_count = inner.weak.count as _;
    }

    pub(crate) fn populate_debug_info(
        &self,
        out: &mut BinderNodeDebugInfo,
        guard: &Guard<'_, Mutex<ProcessInner>>,
    ) {
        out.ptr = self.ptr as _;
        out.cookie = self.cookie as _;
        let inner = self.inner.access(guard);
        if inner.strong.has_count {
            out.has_strong_ref = 1;
        }
        if inner.weak.has_count {
            out.has_weak_ref = 1;
        }
    }

    pub(crate) fn force_has_count(&self, guard: &mut Guard<'_, Mutex<ProcessInner>>) {
        let inner = self.inner.access_mut(guard);
        inner.strong.has_count = true;
        inner.weak.has_count = true;
    }

    fn write(&self, writer: &mut UserSlicePtrWriter, code: u32) -> Result {
        writer.write(&code)?;
        writer.write(&self.ptr)?;
        writer.write(&self.cookie)?;
        Ok(())
    }
}

impl DeliverToRead for Node {
    fn do_work(self: Arc<Self>, _thread: &Thread, writer: &mut UserSlicePtrWriter) -> Result<bool> {
        let mut owner_inner = self.owner.inner.lock();
        let inner = self.inner.access_mut(&mut owner_inner);
        let strong = inner.strong.count > 0;
        let has_strong = inner.strong.has_count;
        let weak = strong || inner.weak.count > 0;
        let has_weak = inner.weak.has_count;
        inner.weak.has_count = weak;
        inner.strong.has_count = strong;

        if !weak {
            // Remove the node if there are no references to it.
            owner_inner.remove_node(self.ptr);
        } else {
            if !has_weak {
                inner.weak.add_bias();
            }

            if !has_strong && strong {
                inner.strong.add_bias();
            }
        }

        drop(owner_inner);

        // This could be done more compactly but we write out all the posibilities for
        // compatibility with the original implementation wrt the order of events.
        if weak && !has_weak {
            self.write(writer, BR_INCREFS)?;
        }

        if strong && !has_strong {
            self.write(writer, BR_ACQUIRE)?;
        }

        if !strong && has_strong {
            self.write(writer, BR_RELEASE)?;
        }

        if !weak && has_weak {
            self.write(writer, BR_DECREFS)?;
        }

        Ok(true)
    }

    fn get_links(&self) -> &Links<dyn DeliverToRead> {
        &self.links
    }
}

pub(crate) struct NodeRef {
    pub(crate) node: Arc<Node>,
    strong_count: usize,
    weak_count: usize,
}

impl NodeRef {
    pub(crate) fn new(node: Arc<Node>, strong_count: usize, weak_count: usize) -> Self {
        Self {
            node,
            strong_count,
            weak_count,
        }
    }

    pub(crate) fn absorb(&mut self, mut other: Self) {
        self.strong_count += other.strong_count;
        self.weak_count += other.weak_count;
        other.strong_count = 0;
        other.weak_count = 0;
    }

    pub(crate) fn clone(&self, strong: bool) -> BinderResult<NodeRef> {
        if strong && self.strong_count == 0 {
            return Err(BinderError::new_failed());
        }

        Ok(self
            .node
            .owner
            .inner
            .lock()
            .new_node_ref(self.node.clone(), strong, None))
    }

    /// Updates (increments or decrements) the number of references held against the node. If the
    /// count being updated transitions from 0 to 1 or from 1 to 0, the node is notified by having
    /// its `update_refcount` function called.
    ///
    /// Returns whether `self` should be removed (when both counts are zero).
    pub(crate) fn update(&mut self, inc: bool, strong: bool) -> bool {
        if strong && self.strong_count == 0 {
            return false;
        }

        let (count, other_count) = if strong {
            (&mut self.strong_count, self.weak_count)
        } else {
            (&mut self.weak_count, self.strong_count)
        };

        if inc {
            if *count == 0 {
                self.node.update_refcount(true, strong);
            }
            *count += 1;
        } else {
            *count -= 1;
            if *count == 0 {
                self.node.update_refcount(false, strong);
                return other_count == 0;
            }
        }

        false
    }
}

impl Drop for NodeRef {
    fn drop(&mut self) {
        if self.strong_count > 0 {
            self.node.update_refcount(false, true);
        }

        if self.weak_count > 0 {
            self.node.update_refcount(false, false);
        }
    }
}
