// SPDX-License-Identifier: GPL-2.0

//! Kernel support for executing futures in C workqueues (`struct workqueue_struct`).

use super::{ArcWake, AutoStopHandle};
use crate::{
    error::code::*,
    macros::pin_project,
    new_mutex, pin_init,
    prelude::*,
    revocable::AsyncRevocable,
    sync::{Arc, ArcBorrow, LockClassKey, Mutex, UniqueArc},
    unsafe_list,
    workqueue::{BoxedQueue, Queue, Work, WorkAdapter},
    Either, Left, Result, Right,
};
use core::{cell::UnsafeCell, future::Future, marker::PhantomPinned, pin::Pin, task::Context};

trait RevocableTask {
    fn revoke(&self);
    fn flush(self: Arc<Self>);
    fn to_links(&self) -> &unsafe_list::Links<dyn RevocableTask>;
}

// SAFETY: `Task` has a single `links` field and only one adapter.
unsafe impl unsafe_list::Adapter for dyn RevocableTask {
    type EntryType = dyn RevocableTask;
    fn to_links(obj: &dyn RevocableTask) -> &unsafe_list::Links<dyn RevocableTask> {
        obj.to_links()
    }
}

struct Task<T: 'static + Send + Future> {
    links: unsafe_list::Links<dyn RevocableTask>,
    executor: Arc<Executor>,
    work: Work,
    future: AsyncRevocable<UnsafeCell<T>>,
}

// SAFETY: The `future` field is only used by one thread at a time (in the `poll` method, which is
// called by the work queue, who guarantees no reentrancy), so a task is `Sync` as long as the
// future is `Send`.
unsafe impl<T: 'static + Send + Future> Sync for Task<T> {}

// SAFETY: If the future `T` is `Send`, so is the task.
unsafe impl<T: 'static + Send + Future> Send for Task<T> {}

impl<T: 'static + Send + Future> Task<T> {
    fn try_new(
        executor: Arc<Executor>,
        key: &'static LockClassKey,
        future: T,
    ) -> Result<Arc<Self>> {
        let task = UniqueArc::try_new(Self {
            executor: executor.clone(),
            links: unsafe_list::Links::new(),
            // SAFETY: `work` is initialised below.
            work: unsafe { Work::new() },
            future: AsyncRevocable::new(UnsafeCell::new(future)),
        })?;

        Work::init(&task, key);

        let task = Arc::from(task);

        // Add task to list.
        {
            let mut guard = executor.inner.lock();
            if guard.stopped {
                return Err(EINVAL);
            }

            // Convert one reference into a pointer so that we hold on to a ref count while the
            // task is in the list.
            Arc::into_raw(task.clone());

            // SAFETY: The task was just created, so it is not in any other lists. It remains alive
            // because we incremented the refcount to account for it being in the list. It never
            // moves because it's pinned behind a `Arc`.
            unsafe { guard.tasks.push_back(&*task) };
        }

        Ok(task)
    }
}

unsafe impl<T: 'static + Send + Future> WorkAdapter for Task<T> {
    type Target = Self;
    const FIELD_OFFSET: isize = crate::offset_of!(Self, work);
    fn run(task: Arc<Task<T>>) {
        let waker = super::ref_waker(task.clone());
        let mut ctx = Context::from_waker(&waker);

        let guard = if let Some(g) = task.future.try_access() {
            g
        } else {
            return;
        };

        // SAFETY: `future` is pinned when the task is. The task is pinned because it's behind a
        // `Arc`, which is always pinned.
        //
        // Work queues guarantee no reentrancy and this is the only place where the future is
        // dereferenced, so it's ok to do it mutably.
        let future = unsafe { Pin::new_unchecked(&mut *guard.get()) };
        if future.poll(&mut ctx).is_ready() {
            drop(guard);
            task.revoke();
        }
    }
}

impl<T: 'static + Send + Future> super::Task for Task<T> {
    fn sync_stop(self: Arc<Self>) {
        self.revoke();
        self.flush();
    }
}

impl<T: 'static + Send + Future> RevocableTask for Task<T> {
    fn revoke(&self) {
        if !self.future.revoke() {
            // Nothing to do if the task was already revoked.
            return;
        }

        // SAFETY: The object is inserted into the list on creation and only removed when the
        // future is first revoked. (Subsequent revocations don't result in additional attempts
        // to remove per the check above.)
        unsafe { self.executor.inner.lock().tasks.remove(self) };

        // Decrement the refcount now that the task is no longer in the list.
        //
        // SAFETY: `into_raw` was called from `try_new` when the task was added to the list.
        unsafe { Arc::from_raw(self) };
    }

    fn flush(self: Arc<Self>) {
        self.work.cancel();
    }

    fn to_links(&self) -> &unsafe_list::Links<dyn RevocableTask> {
        &self.links
    }
}

impl<T: 'static + Send + Future> ArcWake for Task<T> {
    fn wake(self: Arc<Self>) {
        if self.future.is_revoked() {
            return;
        }

        match &self.executor.queue {
            Left(q) => &**q,
            Right(q) => *q,
        }
        .enqueue(self.clone());
    }

    fn wake_by_ref(self: ArcBorrow<'_, Self>) {
        Arc::from(self).wake();
    }
}

struct ExecutorInner {
    stopped: bool,
    tasks: unsafe_list::List<dyn RevocableTask>,
}

/// An executor backed by a work queue.
///
/// # Examples
///
/// The following example runs two tasks on the shared system workqueue.
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::kasync::executor::workqueue::Executor;
/// use kernel::spawn_task;
/// use kernel::workqueue;
///
/// let mut handle = Executor::try_new(workqueue::system())?;
/// spawn_task!(handle.executor(), async {
///     pr_info!("First workqueue task\n");
/// })?;
/// spawn_task!(handle.executor(), async {
///     pr_info!("Second workqueue task\n");
/// })?;
/// handle.detach();
///
/// # Ok::<(), Error>(())
/// ```
#[pin_project]
pub struct Executor {
    queue: Either<BoxedQueue, &'static Queue>,
    #[pin]
    inner: Mutex<ExecutorInner>,
    #[pin]
    _pin: PhantomPinned,
}

// SAFETY: The executor is backed by a kernel `struct workqueue_struct`, which works from any
// thread.
unsafe impl Send for Executor {}

// SAFETY: The executor is backed by a kernel `struct workqueue_struct`, which can be used
// concurrently by multiple threads.
unsafe impl Sync for Executor {}

impl Executor {
    /// Creates a new workqueue-based executor using a static work queue.
    pub fn try_new(wq: &'static Queue) -> Result<AutoStopHandle<Self>> {
        Self::new_internal(Right(wq))
    }

    /// Creates a new workqueue-based executor using an owned (boxed) work queue.
    pub fn try_new_owned(wq: BoxedQueue) -> Result<AutoStopHandle<Self>> {
        Self::new_internal(Left(wq))
    }

    /// Creates a new workqueue-based executor.
    ///
    /// It uses the given work queue to run its tasks.
    fn new_internal(queue: Either<BoxedQueue, &'static Queue>) -> Result<AutoStopHandle<Self>> {
        Ok(AutoStopHandle::new(
            UniqueArc::pin_init(pin_init!(Self {
                inner: new_mutex!(
                    ExecutorInner {
                        stopped: false,
                        tasks: unsafe_list::List::new()
                    },
                    "Executor::inner"
                ),
                queue,
                _pin: PhantomPinned,
            }))?
            .into(),
        ))
    }
}

impl super::Executor for Executor {
    fn spawn(
        self: ArcBorrow<'_, Self>,
        key: &'static LockClassKey,
        future: impl Future + 'static + Send,
    ) -> Result<Arc<dyn super::Task>> {
        let task = Task::try_new(self.into(), key, future)?;
        task.clone().wake();
        Ok(task)
    }

    fn stop(&self) {
        // Set the `stopped` flag.
        self.inner.lock().stopped = true;

        // Go through all tasks and revoke & flush them.
        //
        // N.B. If we decide to allow "asynchronous" stops, we need to ensure that tasks that have
        // been revoked but not flushed yet remain in the list so that we can flush them here.
        // Otherwise we may have a race where we may have a running task (was revoked while
        // running) that isn't the list anymore, so we think we've synchronously stopped all tasks
        // when we haven't really -- unloading a module in this situation leads to memory safety
        // issues (running unloaded code).
        loop {
            let guard = self.inner.lock();

            let front = if let Some(t) = guard.tasks.front() {
                t
            } else {
                break;
            };

            // Get a new reference to the task.
            //
            // SAFETY: We know all entries in the list are of type `Arc<dyn RevocableTask>` and
            // that a reference exists while the entry is in the list, and since we are holding the
            // list lock, we know it cannot go away. The `into_raw` call below ensures that we
            // don't decrement the refcount accidentally.
            let tasktmp = unsafe { Arc::<dyn RevocableTask>::from_raw(front.as_ptr()) };
            let task = tasktmp.clone();
            Arc::into_raw(tasktmp);

            // Release the mutex before revoking the task.
            drop(guard);

            task.revoke();
            task.flush();
        }
    }
}
