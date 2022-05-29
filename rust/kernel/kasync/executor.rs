// SPDX-License-Identifier: GPL-2.0

//! Kernel support for executing futures.

use crate::{
    linked_list::{GetLinks, GetLinksWrapped, Links},
    sync::{Ref, RefBorrow},
    types::PointerWrapper,
    Result,
};
use core::future::Future;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

pub mod local;

/// An environment for executing tasks.
pub trait Executor {
    /// Starts executing a task defined by the given future.
    fn spawn(self: RefBorrow<'_, Self>, future: impl Future + 'static + Send) -> Result;
}

struct TaskListAdapter;
impl GetLinks for TaskListAdapter {
    type EntryType = dyn Task;
    fn get_links(data: &dyn Task) -> &Links<dyn Task> {
        data.get_links()
    }
}

impl GetLinksWrapped for TaskListAdapter {
    type Wrapped = Ref<dyn Task>;
}

trait Task: Send {
    /// Returns a waker for this task.
    fn waker(self: Ref<Self>) -> Waker;

    /// Polls the task.
    ///
    /// # Safety
    ///
    /// The caller must ensure that only one thread at a time calls this function.
    unsafe fn poll(self: Ref<Self>, cx: &mut Context<'_>) -> Poll<()>;

    /// Returns the linked-list links for this task.
    fn get_links(&self) -> &Links<dyn Task>;
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
