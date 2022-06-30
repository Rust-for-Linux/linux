// SPDX-License-Identifier: GPL-2.0

//! Tasks (threads and processes).
//!
//! C header: [`include/linux/sched.h`](../../../../include/linux/sched.h).

use crate::{
    bindings, c_str, error::from_kernel_err_ptr, types::PointerWrapper, ARef, AlwaysRefCounted,
    Result, ScopeGuard,
};
use alloc::boxed::Box;
use core::{cell::UnsafeCell, fmt, marker::PhantomData, ops::Deref, ptr};

/// Wraps the kernel's `struct task_struct`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `get_task_struct` ensures
/// that the allocation remains valid at least until the matching call to `put_task_struct`.
///
/// # Examples
///
/// The following is an example of getting the PID of the current thread with zero additional cost
/// when compared to the C version:
///
/// ```
/// use kernel::task::Task;
///
/// let pid = Task::current().pid();
/// ```
///
/// Getting the PID of the current process, also zero additional cost:
///
/// ```
/// use kernel::task::Task;
///
/// let pid = Task::current().group_leader().pid();
/// ```
///
/// Getting the current task and storing it in some struct. The reference count is automatically
/// incremented when creating `State` and decremented when it is dropped:
///
/// ```
/// use kernel::{ARef, task::Task};
///
/// struct State {
///     creator: ARef<Task>,
///     index: u32,
/// }
///
/// impl State {
///     fn new() -> Self {
///         Self {
///             creator: Task::current().into(),
///             index: 0,
///         }
///     }
/// }
/// ```
#[repr(transparent)]
pub struct Task(pub(crate) UnsafeCell<bindings::task_struct>);

// SAFETY: It's OK to access `Task` through references from other threads because we're either
// accessing properties that don't change (e.g., `pid`, `group_leader`) or that are properly
// synchronised by C code (e.g., `signal_pending`).
unsafe impl Sync for Task {}

/// The type of process identifiers (PIDs).
type Pid = bindings::pid_t;

impl Task {
    /// Returns a task reference for the currently executing task/thread.
    pub fn current<'a>() -> TaskRef<'a> {
        // SAFETY: Just an FFI call.
        let ptr = unsafe { bindings::get_current() };

        TaskRef {
            // SAFETY: If the current thread is still running, the current task is valid. Given
            // that `TaskRef` is not `Send`, we know it cannot be transferred to another thread
            // (where it could potentially outlive the caller).
            task: unsafe { &*ptr.cast() },
            _not_send: PhantomData,
        }
    }

    /// Returns the group leader of the given task.
    pub fn group_leader(&self) -> &Task {
        // SAFETY: By the type invariant, we know that `self.0` is valid.
        let ptr = unsafe { core::ptr::addr_of!((*self.0.get()).group_leader).read() };

        // SAFETY: The lifetime of the returned task reference is tied to the lifetime of `self`,
        // and given that a task has a reference to its group leader, we know it must be valid for
        // the lifetime of the returned task reference.
        unsafe { &*ptr.cast() }
    }

    /// Returns the PID of the given task.
    pub fn pid(&self) -> Pid {
        // SAFETY: By the type invariant, we know that `self.0` is valid.
        unsafe { core::ptr::addr_of!((*self.0.get()).pid).read() }
    }

    /// Determines whether the given task has pending signals.
    pub fn signal_pending(&self) -> bool {
        // SAFETY: By the type invariant, we know that `self.0` is valid.
        unsafe { bindings::signal_pending(self.0.get()) != 0 }
    }

    /// Starts a new kernel thread and runs it.
    ///
    /// # Examples
    ///
    /// Launches 10 threads and waits for them to complete.
    ///
    /// ```
    /// use kernel::task::Task;
    /// use kernel::sync::{CondVar, Mutex};
    /// use core::sync::atomic::{AtomicU32, Ordering};
    ///
    /// kernel::init_static_sync! {
    ///     static COUNT: Mutex<u32> = 0;
    ///     static COUNT_IS_ZERO: CondVar;
    /// }
    ///
    /// fn threadfn() {
    ///     pr_info!("Running from thread {}\n", Task::current().pid());
    ///     let mut guard = COUNT.lock();
    ///     *guard -= 1;
    ///     if *guard == 0 {
    ///         COUNT_IS_ZERO.notify_all();
    ///     }
    /// }
    ///
    /// // Set count to 10 and spawn 10 threads.
    /// *COUNT.lock() = 10;
    /// for i in 0..10  {
    ///     Task::spawn(fmt!("test{i}"), threadfn).unwrap();
    /// }
    ///
    /// // Wait for count to drop to zero.
    /// let mut guard = COUNT.lock();
    /// while (*guard != 0) {
    ///     COUNT_IS_ZERO.wait(&mut guard);
    /// }
    /// ```
    pub fn spawn<T: FnOnce() + Send + 'static>(
        name: fmt::Arguments<'_>,
        func: T,
    ) -> Result<ARef<Task>> {
        unsafe extern "C" fn threadfn<T: FnOnce() + Send + 'static>(
            arg: *mut core::ffi::c_void,
        ) -> core::ffi::c_int {
            // SAFETY: The thread argument is always a `Box<T>` because it is only called via the
            // thread creation below.
            let bfunc = unsafe { Box::<T>::from_pointer(arg) };
            bfunc();
            0
        }

        let arg = Box::try_new(func)?.into_pointer();

        // SAFETY: `arg` was just created with a call to `into_pointer` above.
        let guard = ScopeGuard::new(|| unsafe {
            Box::<T>::from_pointer(arg);
        });

        // SAFETY: The function pointer is always valid (as long as the module remains loaded).
        // Ownership of `raw` is transferred to the new thread (if one is actually created), so it
        // remains valid. Lastly, the C format string is a constant that require formatting as the
        // one and only extra argument.
        let ktask = from_kernel_err_ptr(unsafe {
            bindings::kthread_create_on_node(
                Some(threadfn::<T>),
                arg as _,
                bindings::NUMA_NO_NODE,
                c_str!("%pA").as_char_ptr(),
                &name as *const _ as *const core::ffi::c_void,
            )
        })?;

        // SAFETY: Since the kthread creation succeeded and we haven't run it yet, we know the task
        // is valid.
        let task = unsafe { &*(ktask as *const Task) }.into();

        // SAFETY: Since the kthread creation succeeded, we know `ktask` is valid.
        unsafe { bindings::wake_up_process(ktask) };
        guard.dismiss();
        Ok(task)
    }
}

// SAFETY: The type invariants guarantee that `Task` is always ref-counted.
unsafe impl AlwaysRefCounted for Task {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_task_struct(self.0.get()) };
    }

    unsafe fn dec_ref(obj: ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::put_task_struct(obj.cast().as_ptr()) }
    }
}

/// A wrapper for a shared reference to [`Task`] that isn't [`Send`].
///
/// We make this explicitly not [`Send`] so that we can use it to represent the current thread
/// without having to increment/decrement the task's reference count.
///
/// # Invariants
///
/// The wrapped [`Task`] remains valid for the lifetime of the object.
pub struct TaskRef<'a> {
    task: &'a Task,
    _not_send: PhantomData<*mut ()>,
}

impl Deref for TaskRef<'_> {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        self.task
    }
}

impl From<TaskRef<'_>> for ARef<Task> {
    fn from(t: TaskRef<'_>) -> Self {
        t.deref().into()
    }
}
