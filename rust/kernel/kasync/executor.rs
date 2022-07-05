// SPDX-License-Identifier: GPL-2.0

//! Kernel support for executing futures.

use crate::{
    sync::{LockClassKey, Ref, RefBorrow},
    types::PointerWrapper,
    Result,
};
use core::{
    future::Future,
    task::{RawWaker, RawWakerVTable, Waker},
};

pub mod workqueue;

/// Spawns a new task to run in the given executor.
///
/// It also automatically defines a new lockdep lock class for executors (e.g., workqueue) that
/// require one.
#[macro_export]
macro_rules! spawn_task {
    ($executor:expr, $task:expr) => {{
        static CLASS: $crate::sync::LockClassKey = $crate::sync::LockClassKey::new();
        $crate::kasync::executor::Executor::spawn($executor, &CLASS, $task)
    }};
}

/// A task spawned in an executor.
pub trait Task {
    /// Synchronously stops the task.
    ///
    /// It ensures that the task won't run again and releases resources needed to run the task
    /// (e.g., the closure is dropped). If the task is inflight, it waits for the task to block or
    /// complete before cleaning up and returning.
    ///
    /// Callers must not call this from within the task itself as it will likely lead to a
    /// deadlock.
    fn sync_stop(self: Ref<Self>);
}

/// An environment for executing tasks.
pub trait Executor: Sync + Send {
    /// Starts executing a task defined by the given future.
    ///
    /// Callers are encouraged to use the [`spawn_task`] macro because it automatically defines a
    /// new lock class key.
    fn spawn(
        self: RefBorrow<'_, Self>,
        lock_class_key: &'static LockClassKey,
        future: impl Future + 'static + Send,
    ) -> Result<Ref<dyn Task>>
    where
        Self: Sized;

    /// Stops the executor.
    ///
    /// After it is called, attempts to spawn new tasks will result in an error and existing ones
    /// won't be polled anymore.
    fn stop(&self);
}

/// A waker that is wrapped in [`Ref`] for its reference counting.
///
/// Types that implement this trait can get a [`Waker`] by calling [`ref_waker`].
pub trait RefWake: Send + Sync {
    /// Wakes a task up.
    fn wake_by_ref(self: RefBorrow<'_, Self>);

    /// Wakes a task up and consumes a reference.
    fn wake(self: Ref<Self>) {
        self.as_ref_borrow().wake_by_ref();
    }
}

/// Creates a [`Waker`] from a [`Ref<T>`], where `T` implements the [`RefWake`] trait.
pub fn ref_waker<T: 'static + RefWake>(w: Ref<T>) -> Waker {
    fn raw_waker<T: 'static + RefWake>(w: Ref<T>) -> RawWaker {
        let data = w.into_pointer();
        RawWaker::new(
            data.cast(),
            &RawWakerVTable::new(clone::<T>, wake::<T>, wake_by_ref::<T>, drop::<T>),
        )
    }

    unsafe fn clone<T: 'static + RefWake>(ptr: *const ()) -> RawWaker {
        // SAFETY: The data stored in the raw waker is the result of a call to `into_pointer`.
        let w = unsafe { Ref::<T>::borrow(ptr.cast()) };
        raw_waker(w.into())
    }

    unsafe fn wake<T: 'static + RefWake>(ptr: *const ()) {
        // SAFETY: The data stored in the raw waker is the result of a call to `into_pointer`.
        let w = unsafe { Ref::<T>::from_pointer(ptr.cast()) };
        w.wake();
    }

    unsafe fn wake_by_ref<T: 'static + RefWake>(ptr: *const ()) {
        // SAFETY: The data stored in the raw waker is the result of a call to `into_pointer`.
        let w = unsafe { Ref::<T>::borrow(ptr.cast()) };
        w.wake_by_ref();
    }

    unsafe fn drop<T: 'static + RefWake>(ptr: *const ()) {
        // SAFETY: The data stored in the raw waker is the result of a call to `into_pointer`.
        unsafe { Ref::<T>::from_pointer(ptr.cast()) };
    }

    let raw = raw_waker(w);
    // SAFETY: The vtable of the raw waker satisfy the behaviour requirements of a waker.
    unsafe { Waker::from_raw(raw) }
}

/// A handle to an executor that automatically stops it on drop.
pub struct AutoStopHandle<T: Executor + ?Sized> {
    executor: Option<Ref<T>>,
}

impl<T: Executor + ?Sized> AutoStopHandle<T> {
    /// Creates a new instance of an [`AutoStopHandle`].
    pub fn new(executor: Ref<T>) -> Self {
        Self {
            executor: Some(executor),
        }
    }

    /// Detaches from the auto-stop handle.
    ///
    /// That is, extracts the executor from the handle and doesn't stop it anymore.
    pub fn detach(mut self) -> Ref<T> {
        self.executor.take().unwrap()
    }

    /// Returns the executor associated with the auto-stop handle.
    ///
    /// This is so that callers can, for example, spawn new tasks.
    pub fn executor(&self) -> RefBorrow<'_, T> {
        self.executor.as_ref().unwrap().as_ref_borrow()
    }
}

impl<T: Executor + ?Sized> Drop for AutoStopHandle<T> {
    fn drop(&mut self) {
        if let Some(ex) = self.executor.take() {
            ex.stop();
        }
    }
}

impl<T: 'static + Executor> From<AutoStopHandle<T>> for AutoStopHandle<dyn Executor> {
    fn from(src: AutoStopHandle<T>) -> Self {
        Self::new(src.detach())
    }
}
