// SPDX-License-Identifier: GPL-2.0

//! Work queues.
//!
//! C header: [`include/linux/workqueue.h`](../../../../include/linux/workqueue.h)

use crate::{
    bindings, c_str,
    error::code::*,
    sync::{Arc, ArcBorrow, LockClassKey, UniqueArc},
    Opaque, Result,
};
use core::{fmt, ops::Deref, ptr::NonNull};

/// Spawns a new work item to run in the work queue.
///
/// It also automatically defines a new lockdep lock class for the work item.
#[macro_export]
macro_rules! spawn_work_item {
    ($queue:expr, $func:expr) => {{
        static CLASS: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        $crate::workqueue::Queue::try_spawn($queue, &CLASS, $func)
    }};
}

/// Implements the [`WorkAdapter`] trait for a type where its [`Work`] instance is a field.
///
/// # Examples
///
/// ```
/// # use kernel::workqueue::Work;
///
/// struct Example {
///     work: Work,
/// }
///
/// kernel::impl_self_work_adapter!(Example, work, |_| {});
/// ```
#[macro_export]
macro_rules! impl_self_work_adapter {
    ($work_type:ty, $field:ident, $closure:expr) => {
        $crate::impl_work_adapter!($work_type, $work_type, $field, $closure);
    };
}

/// Implements the [`WorkAdapter`] trait for an adapter type.
///
/// # Examples
///
/// ```
/// # use kernel::workqueue::Work;
///
/// struct Example {
///     work: Work,
/// }
///
/// struct Adapter;
///
/// kernel::impl_work_adapter!(Adapter, Example, work, |_| {});
/// ```
#[macro_export]
macro_rules! impl_work_adapter {
    ($adapter:ty, $work_type:ty, $field:ident, $closure:expr) => {
        // SAFETY: We use `offset_of` to ensure that the field is within the given type, and we
        // also check its type is `Work`.
        unsafe impl $crate::workqueue::WorkAdapter for $adapter {
            type Target = $work_type;
            const FIELD_OFFSET: isize = $crate::offset_of!(Self::Target, $field);
            fn run(w: $crate::sync::Arc<Self::Target>) {
                let closure: fn($crate::sync::Arc<Self::Target>) = $closure;
                closure(w);
                return;

                // Checks that the type of the field is actually `Work`.
                let tmp = core::mem::MaybeUninit::<$work_type>::uninit();
                // SAFETY: The pointer is valid and aligned, just not initialised; `addr_of`
                // ensures that we don't actually read from it (which would be UB) nor create an
                // intermediate reference.
                let _x: *const $crate::workqueue::Work =
                    unsafe { core::ptr::addr_of!((*tmp.as_ptr()).$field) };
            }
        }
    };
}

/// Initialises a work item.
///
/// It automatically defines a new lockdep lock class for the work item.
#[macro_export]
macro_rules! init_work_item {
    ($work_container:expr) => {{
        static CLASS: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        $crate::workqueue::Work::init($work_container, &CLASS)
    }};
}

/// Initialises a work item with the given adapter.
///
/// It automatically defines a new lockdep lock class for the work item.
#[macro_export]
macro_rules! init_work_item_adapter {
    ($adapter:ty, $work_container:expr) => {{
        static CLASS: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        $crate::workqueue::Work::init_with_adapter::<$adapter>($work_container, &CLASS)
    }};
}

/// A kernel work queue.
///
/// Wraps the kernel's C `struct workqueue_struct`.
///
/// It allows work items to be queued to run on thread pools managed by the kernel. Several are
/// always available, for example, the ones returned by [`system`], [`system_highpri`],
/// [`system_long`], etc.
///
/// # Examples
///
/// The following example is the simplest way to launch a work item:
///
/// ```
/// # use kernel::{spawn_work_item, workqueue};
/// spawn_work_item!(workqueue::system(), || pr_info!("Hello from a work item\n"))?;
///
/// # Ok::<(), Error>(())
/// ```
///
/// The following example is used to create a work item and enqueue it several times. We note that
/// enqueuing while the work item is already queued is a no-op, so we enqueue it when it is not
/// enqueued yet.
///
/// ```
/// # use kernel::workqueue::{self, Work};
/// use core::sync::atomic::{AtomicU32, Ordering};
/// use kernel::sync::UniqueArc;
///
/// struct Example {
///     count: AtomicU32,
///     work: Work,
/// }
///
/// kernel::impl_self_work_adapter!(Example, work, |w| {
///     let count = w.count.fetch_add(1, Ordering::Relaxed);
///     pr_info!("Called with count={}\n", count);
///
///     // Queue again if the count is less than 10.
///     if count < 10 {
///         workqueue::system().enqueue(w);
///     }
/// });
///
/// let e = UniqueArc::try_new(Example {
///     count: AtomicU32::new(0),
///     // SAFETY: `work` is initialised below.
///     work: unsafe { Work::new() },
/// })?;
///
/// kernel::init_work_item!(&e);
///
/// // Queue the first time.
/// workqueue::system().enqueue(e.into());
///
/// # Ok::<(), Error>(())
/// ```
///
/// The following example has two different work items in the same struct, which allows it to be
/// queued twice.
///
/// ```
/// # use kernel::workqueue::{self, Work, WorkAdapter};
/// use core::sync::atomic::{AtomicU32, Ordering};
/// use kernel::sync::{Arc, UniqueArc};
///
/// struct Example {
///     work1: Work,
///     work2: Work,
/// }
///
/// kernel::impl_self_work_adapter!(Example, work1, |_| pr_info!("First work\n"));
///
/// struct SecondAdapter;
/// kernel::impl_work_adapter!(SecondAdapter, Example, work2, |_| pr_info!("Second work\n"));
///
/// let e = UniqueArc::try_new(Example {
///     // SAFETY: `work1` is initialised below.
///     work1: unsafe { Work::new() },
///     // SAFETY: `work2` is initialised below.
///     work2: unsafe { Work::new() },
/// })?;
///
/// kernel::init_work_item!(&e);
/// kernel::init_work_item_adapter!(SecondAdapter, &e);
///
/// let e = Arc::from(e);
///
/// // Enqueue the two different work items.
/// workqueue::system().enqueue(e.clone());
/// workqueue::system().enqueue_adapter::<SecondAdapter>(e);
///
/// # Ok::<(), Error>(())
/// ```
#[repr(transparent)]
pub struct Queue(Opaque<bindings::workqueue_struct>);

// SAFETY: Kernel workqueues are usable from any thread.
unsafe impl Sync for Queue {}

impl Queue {
    /// Tries to allocate a new work queue.
    ///
    /// Callers should first consider using one of the existing ones (e.g. [`system`]) before
    /// deciding to create a new one.
    pub fn try_new(name: fmt::Arguments<'_>) -> Result<BoxedQueue> {
        // SAFETY: We use a format string that requires an `fmt::Arguments` pointer as the first
        // and only argument.
        let ptr = unsafe {
            bindings::alloc_workqueue(
                c_str!("%pA").as_char_ptr(),
                0,
                0,
                &name as *const _ as *const core::ffi::c_void,
            )
        };
        if ptr.is_null() {
            return Err(ENOMEM);
        }

        // SAFETY: `ptr` was just allocated and checked above, so it non-null and valid. Plus, it
        // isn't touched after the call below, so ownership is transferred.
        Ok(unsafe { BoxedQueue::new(ptr) })
    }

    /// Enqueues a work item.
    ///
    /// Returns `true` if the work item was successfully enqueue; returns `false` if it had already
    /// been (and continued to be) enqueued.
    pub fn enqueue<T: WorkAdapter<Target = T>>(&self, w: ArcBorrow<'_, T>) -> bool {
        self.enqueue_adapter::<T>(w)
    }

    /// Enqueues a work item with an explicit adapter.
    ///
    /// Returns `true` if the work item was successfully enqueue; returns `false` if it had already
    /// been (and continued to be) enqueued.
    pub fn enqueue_adapter<A: WorkAdapter + ?Sized>(&self, w: ArcBorrow<'_, A::Target>) -> bool {
        let ptr = &*w as *const A::Target;
        let field_ptr =
            (ptr as *const u8).wrapping_offset(A::FIELD_OFFSET) as *mut bindings::work_struct;

        // SAFETY: Having a shared reference to work queue guarantees that it remains valid, while
        // the work item remains valid because we will increment the reference count later if this function
        // returns true.
        let ret = unsafe {
            bindings::queue_work_on(bindings::WORK_CPU_UNBOUND as _, self.0.get(), field_ptr)
        };

        if ret {
            // Increase the reference count because the work item has a copy.
            core::mem::forget(Arc::from(w));
        }

        ret
    }

    /// Tries to spawn the given function or closure as a work item.
    ///
    /// Users are encouraged to use [`spawn_work_item`] as it automatically defines the lock class
    /// key to be used.
    pub fn try_spawn<T: 'static + Send + Fn()>(
        &self,
        key: &'static LockClassKey,
        func: T,
    ) -> Result {
        let w = UniqueArc::<ClosureAdapter<T>>::try_new(ClosureAdapter {
            // SAFETY: `work` is initialised below.
            work: unsafe { Work::new() },
            func,
        })?;
        Work::init(&w, key);
        self.enqueue(Arc::from(w).as_arc_borrow());
        Ok(())
    }
}

struct ClosureAdapter<T: Fn() + Send> {
    work: Work,
    func: T,
}

// SAFETY: `ClosureAdapter::work` is of type `Work`.
unsafe impl<T: Fn() + Send> WorkAdapter for ClosureAdapter<T> {
    type Target = Self;
    const FIELD_OFFSET: isize = crate::offset_of!(Self, work);

    fn run(w: Arc<Self::Target>) {
        (w.func)();
    }
}

/// An adapter for work items.
///
/// For the most usual case where a type has a [`Work`] in it and is itself the adapter, it is
/// recommended that they use the [`impl_self_work_adapter`] or [`impl_work_adapter`] macros
/// instead of implementing the [`WorkAdapter`] manually. The great advantage is that they don't
/// require any unsafe blocks.
///
/// # Safety
///
/// Implementers must ensure that there is a [`Work`] instance `FIELD_OFFSET` bytes from the
/// beginning of a valid `Target` type. It is normally safe to use the [`crate::offset_of`] macro
/// for this.
pub unsafe trait WorkAdapter {
    /// The type that this work adapter is meant to use.
    type Target;

    /// The offset, in bytes, from the beginning of [`Self::Target`] to the instance of [`Work`].
    const FIELD_OFFSET: isize;

    /// Runs when the work item is picked up for execution after it has been enqueued to some work
    /// queue.
    fn run(w: Arc<Self::Target>);
}

/// A work item.
///
/// Wraps the kernel's C `struct work_struct`.
///
/// Users must add a field of this type to a structure, then implement [`WorkAdapter`] so that it
/// can be queued for execution in a thread pool. Examples of it being used are available in the
/// documentation for [`Queue`].
#[repr(transparent)]
pub struct Work(Opaque<bindings::work_struct>);

impl Work {
    /// Creates a new instance of [`Work`].
    ///
    /// # Safety
    ///
    /// Callers must call either [`Work::init`] or [`Work::init_with_adapter`] before the work item
    /// can be used.
    pub unsafe fn new() -> Self {
        Self(Opaque::uninit())
    }

    /// Initialises the work item.
    ///
    /// Users should prefer the [`init_work_item`] macro because it automatically defines a new
    /// lock class key.
    pub fn init<T: WorkAdapter<Target = T>>(obj: &UniqueArc<T>, key: &'static LockClassKey) {
        Self::init_with_adapter::<T>(obj, key)
    }

    /// Initialises the work item with the given adapter.
    ///
    /// Users should prefer the [`init_work_item_adapter`] macro because it automatically defines a
    /// new lock class key.
    pub fn init_with_adapter<A: WorkAdapter>(
        obj: &UniqueArc<A::Target>,
        key: &'static LockClassKey,
    ) {
        let ptr = &**obj as *const _ as *const u8;
        let field_ptr = ptr.wrapping_offset(A::FIELD_OFFSET) as *mut bindings::work_struct;

        // SAFETY: `work` is valid for writes -- the `UniqueArc` instance guarantees that it has
        // been allocated and there is only one pointer to it. Additionally, `work_func` is a valid
        // callback for the work item.
        unsafe {
            bindings::__INIT_WORK_WITH_KEY(field_ptr, Some(Self::work_func::<A>), false, key.get())
        };
    }

    /// Cancels the work item.
    ///
    /// It is ok for this to be called when the work is not queued.
    pub fn cancel(&self) {
        // SAFETY: The work is valid (we have a reference to it), and the function can be called
        // whether the work is queued or not.
        if unsafe { bindings::cancel_work_sync(self.0.get()) } {
            // SAFETY: When the work was queued, a call to `into_raw` was made. We just canceled
            // the work without it having the chance to run, so we need to explicitly destroy this
            // reference (which would have happened in `work_func` if it did run).
            #[allow(clippy::borrow_deref_ref)]
            unsafe {
                Arc::from_raw(&*self)
            };
        }
    }

    unsafe extern "C" fn work_func<A: WorkAdapter>(work: *mut bindings::work_struct) {
        let field_ptr = work as *const _ as *const u8;
        let ptr = field_ptr.wrapping_offset(-A::FIELD_OFFSET) as *const A::Target;

        // SAFETY: This callback is only ever used by the `init_with_adapter` method, so it is
        // always the case that the work item is embedded in a `Work` (Self) struct.
        let w = unsafe { Arc::from_raw(ptr) };
        A::run(w);
    }
}

/// A boxed owned workqueue.
///
/// # Invariants
///
/// `ptr` is owned by this instance of [`BoxedQueue`], so it's always valid.
pub struct BoxedQueue {
    ptr: NonNull<Queue>,
}

impl BoxedQueue {
    /// Creates a new instance of [`BoxedQueue`].
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid. Additionally, ownership must be handed over to new
    /// instance of [`BoxedQueue`].
    unsafe fn new(ptr: *mut bindings::workqueue_struct) -> Self {
        Self {
            // SAFETY: We checked above that `ptr` is non-null.
            ptr: unsafe { NonNull::new_unchecked(ptr.cast()) },
        }
    }
}

impl Deref for BoxedQueue {
    type Target = Queue;

    fn deref(&self) -> &Queue {
        // SAFETY: The type invariants guarantee that `ptr` is always valid.
        unsafe { self.ptr.as_ref() }
    }
}

impl Drop for BoxedQueue {
    fn drop(&mut self) {
        // SAFETY: The type invariants guarantee that `ptr` is always valid.
        unsafe { bindings::destroy_workqueue(self.ptr.as_ref().0.get()) };
    }
}

/// Returns the system work queue (`system_wq`).
///
/// It is the one used by schedule\[_delayed\]_work\[_on\](). Multi-CPU multi-threaded. There are
/// users which expect relatively short queue flush time.
///
/// Callers shouldn't queue work items which can run for too long.
pub fn system() -> &'static Queue {
    // SAFETY: `system_wq` is a C global, always available.
    unsafe { &*bindings::system_wq.cast() }
}

/// Returns the system high-priority work queue (`system_highpri_wq`).
///
/// It is similar to the one returned by [`system`] but for work items which require higher
/// scheduling priority.
pub fn system_highpri() -> &'static Queue {
    // SAFETY: `system_highpri_wq` is a C global, always available.
    unsafe { &*bindings::system_highpri_wq.cast() }
}

/// Returns the system work queue for potentially long-running work items (`system_long_wq`).
///
/// It is similar to the one returned by [`system`] but may host long running work items. Queue
/// flushing might take relatively long.
pub fn system_long() -> &'static Queue {
    // SAFETY: `system_long_wq` is a C global, always available.
    unsafe { &*bindings::system_long_wq.cast() }
}

/// Returns the system unbound work queue (`system_unbound_wq`).
///
/// Workers are not bound to any specific CPU, not concurrency managed, and all queued work items
/// are executed immediately as long as `max_active` limit is not reached and resources are
/// available.
pub fn system_unbound() -> &'static Queue {
    // SAFETY: `system_unbound_wq` is a C global, always available.
    unsafe { &*bindings::system_unbound_wq.cast() }
}

/// Returns the system freezable work queue (`system_freezable_wq`).
///
/// It is equivalent to the one returned by [`system`] except that it's freezable.
pub fn system_freezable() -> &'static Queue {
    // SAFETY: `system_freezable_wq` is a C global, always available.
    unsafe { &*bindings::system_freezable_wq.cast() }
}

/// Returns the system power-efficient work queue (`system_power_efficient_wq`).
///
/// It is inclined towards saving power and is converted to "unbound" variants if the
/// `workqueue.power_efficient` kernel parameter is specified; otherwise, it is similar to the one
/// returned by [`system`].
pub fn system_power_efficient() -> &'static Queue {
    // SAFETY: `system_power_efficient_wq` is a C global, always available.
    unsafe { &*bindings::system_power_efficient_wq.cast() }
}

/// Returns the system freezable power-efficient work queue (`system_freezable_power_efficient_wq`).
///
/// It is similar to the one returned by [`system_power_efficient`] except that is freezable.
pub fn system_freezable_power_efficient() -> &'static Queue {
    // SAFETY: `system_freezable_power_efficient_wq` is a C global, always available.
    unsafe { &*bindings::system_freezable_power_efficient_wq.cast() }
}
