// SPDX-License-Identifier: GPL-2.0

//! Kernel support for executing futures in the local thread or a dedicated one.

use super::{Task, TaskListAdapter};
use crate::{
    bindings, c_str, c_types,
    error::{code::*, from_kernel_err_ptr, from_kernel_result},
    linked_list::{Links, List},
    sync::{CondVar, Ref, RefBorrow, SpinLock, UniqueRef},
    types::PointerWrapper,
    Result, ScopeGuard,
};
use core::task::{Context, Poll, Waker};
use core::{cell::UnsafeCell, fmt, future::Future, marker::PhantomPinned, pin::Pin};

struct LocalTask<T: 'static + Send + Future> {
    executor: Ref<Local>,
    future: UnsafeCell<T>,
    links: Links<dyn Task>,
}

// SAFETY: The `future` field is only used by one thread at a time (in the `poll` method, whose
// safety requirements include serialisation), so a local task is `Sync` as long as the future is
// `Send`.
unsafe impl<T: 'static + Send + Future> Sync for LocalTask<T> {}

// SAFETY: If the future `T` is `Send`, so is the local task.
unsafe impl<T: 'static + Send + Future> Send for LocalTask<T> {}

impl<T: 'static + Send + Future> LocalTask<T> {
    fn try_new(executor: Ref<Local>, f: T) -> Result<Ref<Self>> {
        Ref::try_new(Self {
            executor,
            future: UnsafeCell::new(f),
            links: Links::new(),
        })
    }
}

impl<T: 'static + Send + Future> super::RefWake for LocalTask<T> {
    fn wake_by_ref(self: RefBorrow<'_, Self>) {
        {
            let mut inner = self.executor.inner.lock_irqdisable();
            if inner.flags & STOP_REQUESTED != 0 {
                // No need to queue if the executor is stopping.
                return;
            }

            inner.ready_list.push_back(Ref::<Self>::from(self));
        }
        self.executor.condvar.notify_all();
    }
}

impl<T: 'static + Send + Future> Task for LocalTask<T> {
    unsafe fn poll(self: Ref<Self>, cx: &mut Context<'_>) -> Poll<()> {
        // SAFETY: `future` is pinned when the task is. The task is pinned because it's behind a
        // `Ref`, which is always pinned.
        //
        // The safety requirements for this function ensure that only one thread at a time is
        // calling it. This is the only place where the future is dereferenced, so it's ok to do it
        // mutably.
        let future = unsafe { Pin::new_unchecked(&mut *self.future.get()) };
        future.poll(cx).map(|_| ())
    }

    fn waker(self: Ref<Self>) -> Waker {
        super::ref_waker(self)
    }

    fn get_links(&self) -> &Links<dyn Task> {
        &self.links
    }
}

/// Indicates that the executor is running.
const RUNNING: u8 = 0x1;

/// Indicates that the executor was asked to stop.
const STOP_REQUESTED: u8 = 0x2;

struct Inner {
    /// The number of tasks associated with the executor.
    ///
    /// It includes both ready and not-ready tasks, but excludes completed ones.
    count: usize,

    /// Flags pertaining to the executor. They are defined above (e.g., [`STOP_REQUESTED`] and
    /// [`RUNNING`]).
    flags: u8,

    /// The list of tasks that are ready to be polled.
    ready_list: List<TaskListAdapter>,
}

/// An executor that runs tasks on the caller thread or a dedicated one.
///
/// # Examples
///
/// The following example runs two tasks on the local (caller) thread.
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::kasync::executor::{Executor, local::Local};
///
/// fn example_local_thread() -> Result {
///     let mut handle = Local::try_new()?;
///     handle.executor().spawn(async {
///         pr_info!("First task\n");
///     })?;
///     handle.executor().spawn(async {
///         pr_info!("Second task\n");
///     })?;
///     handle.run(false)?;
///     Ok(())
/// }
/// ```
///
/// The following example runs tasks on a dedicated thread that automatically exits when all tasks
/// are completed.
///
/// ```
/// # use kernel::prelude::*;
/// use kernel::kasync::executor::{Executor, local::Local};
///
/// fn example_dedicated_thread() -> Result {
///     let handle = Local::try_new()?;
///     handle.executor().spawn(async {
///         pr_info!("First task\n");
///     })?;
///     handle.executor().spawn(async {
///         pr_info!("Second task\n");
///     })?;
///     handle.run_on_dedicated_thread(true, fmt!("example-thread"))?.detach();
///     Ok(())
/// }
/// ```
pub struct Local {
    inner: SpinLock<Inner>,
    condvar: CondVar,
    _pin: PhantomPinned,
}

impl Local {
    /// Creates a new local executor.
    pub fn try_new() -> Result<RunHandle> {
        let mut e = Pin::from(UniqueRef::try_new(Self {
            // SAFETY: `spinlock_init` is called below.
            inner: unsafe {
                SpinLock::new(Inner {
                    count: 0,
                    flags: 0,
                    ready_list: List::new(),
                })
            },
            // SAFETY: `condvar_init` is called below.
            condvar: unsafe { CondVar::new() },
            _pin: PhantomPinned,
        })?);

        // SAFETY: `inner` is pinned when the executor is.
        let inner = unsafe { e.as_mut().map_unchecked_mut(|e| &mut e.inner) };
        crate::spinlock_init!(inner, "Local::inner");

        // SAFETY: `condvar` is pinned when the executor is.
        let condvar = unsafe { e.as_mut().map_unchecked_mut(|e| &mut e.condvar) };
        crate::condvar_init!(condvar, "Local::condvar");

        Ok(RunHandle { executor: e.into() })
    }

    fn run_loop(&self, stop_on_zero_tasks: bool, interruptible: bool) -> Result {
        loop {
            let mut inner = self.inner.lock_irqdisable();
            while inner.ready_list.is_empty() && inner.flags & STOP_REQUESTED == 0 {
                if !interruptible {
                    self.condvar.wait_uninterruptible(&mut inner);
                } else if self.condvar.wait(&mut inner) {
                    return Err(EINTR);
                }
            }

            if inner.flags & STOP_REQUESTED != 0 {
                // Executor was stopped.
                break;
            }

            if let Some(task) = inner.ready_list.pop_front() {
                drop(inner);
                let waker = task.clone().waker();
                let mut ctx = Context::from_waker(&waker);

                // SAFETY: The `RunHandle` type ensures that only one thread at a time can call
                // this function.
                if unsafe { task.poll(&mut ctx) }.is_ready() {
                    let mut inner = self.inner.lock_irqdisable();
                    inner.count -= 1;
                    if stop_on_zero_tasks && inner.count == 0 {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Runs the executor until it is stopped.
    ///
    /// It may optionally also stop when there are no tasks (runnable or waiting) associated with
    /// it.
    fn run(&self, stop_on_zero_tasks: bool, interruptible: bool) -> Result {
        // Set the running flag only if a stop hasn't been requested yet.
        {
            let mut inner = self.inner.lock_irqdisable();

            if inner.flags & STOP_REQUESTED != 0 {
                return Ok(());
            }

            if stop_on_zero_tasks && inner.count == 0 {
                return Ok(());
            }

            inner.flags |= RUNNING;
        }

        let ret = self.run_loop(stop_on_zero_tasks, interruptible);

        // Reset the running flag and notify any potential joiners.
        let needs_notify;
        {
            let mut inner = self.inner.lock_irqdisable();
            needs_notify = inner.flags & STOP_REQUESTED != 0;
            inner.flags &= !RUNNING;
        }

        if needs_notify {
            self.condvar.notify_all();
        }

        ret
    }

    /// Stops the executor.
    ///
    /// No new tasks can be spawned and no new ones will be polled after [`Self::stop`] is called.
    pub fn stop(&self) {
        {
            let mut inner = self.inner.lock_irqdisable();
            inner.flags |= STOP_REQUESTED;
        }
        self.condvar.notify_all();
    }

    /// Waits for the executor to stop running permanently.
    ///
    /// It returns an [`EINTR`] if the thread wakes up due to a signal.
    pub fn join(&self) -> Result {
        let mut inner = self.inner.lock_irqdisable();
        while inner.flags & (RUNNING | STOP_REQUESTED) != STOP_REQUESTED {
            if self.condvar.wait(&mut inner) {
                return Err(EINTR);
            }
        }
        Ok(())
    }

    /// Waits for the executor to stop running permanently.
    ///
    /// If needed, it sleeps in uninterruptible mode, so the function never fails (but also
    /// continues to wait even if there are pending signals).
    pub fn join_uninterruptible(&self) {
        let mut inner = self.inner.lock_irqdisable();
        while inner.flags & (RUNNING | STOP_REQUESTED) != STOP_REQUESTED {
            self.condvar.wait_uninterruptible(&mut inner);
        }
    }
}

impl super::Executor for Local {
    fn spawn(self: RefBorrow<'_, Self>, future: impl Future + 'static + Send) -> Result {
        let needs_notify;
        let task = LocalTask::try_new(self.into(), future)?;
        {
            let mut inner = self.inner.lock_irqdisable();
            if inner.flags & STOP_REQUESTED != 0 {
                // The executor is already stopped or is stopping.
                return Err(EPIPE);
            }

            needs_notify = inner.ready_list.is_empty();
            inner.ready_list.push_back(task);
            inner.count += 1;
        }

        if needs_notify {
            self.condvar.notify_all();
        }
        Ok(())
    }
}

/// An owned permission to run the local executor.
///
/// This is so that only one thread at a time can run it.
pub struct RunHandle {
    executor: Ref<Local>,
}

impl RunHandle {
    /// Returns the executor associated with the run handle.
    ///
    /// This is so that callers can, for example, spawn new tasks.
    pub fn executor(&self) -> RefBorrow<'_, Local> {
        self.executor.as_ref_borrow()
    }

    /// Runs the executor on the caller's thread, blocking it until it runs to completion or is
    /// interrupted (if `interruptible` is `true`).
    pub fn run(&mut self, interruptible: bool) -> Result {
        self.executor.run(true, interruptible)
    }

    /// Runs he executor on a dedicated thread.
    ///
    /// Differently from [`RunHandle::run`], it doesn't block the caller. Instead, it creates a new
    /// thread (with the given name) and runs the executor there.
    pub fn run_on_dedicated_thread(
        self,
        stop_on_zero_tasks: bool,
        thread_name: fmt::Arguments<'_>,
    ) -> Result<AutoJoinHandle> {
        unsafe extern "C" fn worker<const AUTO_STOP: bool>(
            data: *mut c_types::c_void,
        ) -> c_types::c_int {
            from_kernel_result! {
                // SAFETY: The thread argument is always a `Ref<Local>` because it is only called
                // via the thread creation below.
                let e = unsafe { Ref::<Local>::from_pointer(data) };
                e.run(AUTO_STOP, false)?;
                Ok(0)
            }
        }

        let Self { executor: ex } = self;
        let raw = ex.clone().into_pointer();

        // SAFETY: `raw` was just created with a call to `into_pointer` above.
        let guard = ScopeGuard::new(|| unsafe {
            Ref::<Local>::from_pointer(raw);
        });

        // SAFETY: The function pointer is always valid (as long as the module remains loaded).
        // Ownership of `raw` is transferred to the new thread (if one is actually created), so it
        // remains valid. Lastly, the C format string is a constant that require formatting as the
        // one and only extra argument.
        let ktask = from_kernel_err_ptr(unsafe {
            bindings::kthread_create_on_node(
                Some(if stop_on_zero_tasks {
                    worker::<true>
                } else {
                    worker::<false>
                }),
                raw as _,
                bindings::NUMA_NO_NODE,
                c_str!("%pA").as_char_ptr(),
                &thread_name as *const _ as *const c_types::c_void,
            )
        })?;

        // SAFETY: Since the function call above succeeded, we know `ktask` is valid.
        unsafe { bindings::wake_up_process(ktask) };
        guard.dismiss();
        Ok(AutoJoinHandle { executor: Some(ex) })
    }
}

/// A handle to a local executor that automatically stops and joins (waits for it to complete) the
/// executor on drop.
pub struct AutoJoinHandle {
    executor: Option<Ref<Local>>,
}

impl AutoJoinHandle {
    /// Detaches from the auto-join handle.
    ///
    /// That is, extracts the executor from the handle and prevents it from stopping and joining
    /// the local executor.
    pub fn detach(mut self) -> Ref<Local> {
        self.executor.take().unwrap()
    }

    /// Returns the executor associated with the auto-join handle.
    ///
    /// This is so that callers can, for example, spawn new tasks.
    pub fn executor(&self) -> RefBorrow<'_, Local> {
        self.executor.as_ref().unwrap().as_ref_borrow()
    }
}

impl Drop for AutoJoinHandle {
    fn drop(&mut self) {
        if let Some(ex) = self.executor.take() {
            ex.stop();
            ex.join_uninterruptible();
        }
    }
}
