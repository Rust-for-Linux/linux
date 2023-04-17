// SPDX-License-Identifier: GPL-2.0

//! Async networking.

use crate::{bindings, error::code::*, net, sync::NoWaitLock, types::Opaque, Result};
use core::{
    future::Future,
    marker::{PhantomData, PhantomPinned},
    ops::Deref,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// A socket listening on a TCP port.
///
/// The [`TcpListener::accept`] method is meant to be used in async contexts.
pub struct TcpListener {
    listener: net::TcpListener,
}

impl TcpListener {
    /// Creates a new TCP listener.
    ///
    /// It is configured to listen on the given socket address for the given namespace.
    pub fn try_new(ns: &net::Namespace, addr: &net::SocketAddr) -> Result<Self> {
        Ok(Self {
            listener: net::TcpListener::try_new(ns, addr)?,
        })
    }

    /// Accepts a new connection.
    ///
    /// Returns a future that when ready indicates the result of the accept operation; on success,
    /// it contains the newly-accepted tcp stream.
    pub fn accept(&self) -> impl Future<Output = Result<TcpStream>> + '_ {
        SocketFuture::from_listener(
            self,
            bindings::BINDINGS_EPOLLIN | bindings::BINDINGS_EPOLLERR,
            || {
                Ok(TcpStream {
                    stream: self.listener.accept(false)?,
                })
            },
        )
    }
}

impl Deref for TcpListener {
    type Target = net::TcpListener;

    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

/// A connected TCP socket.
///
/// The potentially blocking methods (e.g., [`TcpStream::read`], [`TcpStream::write`]) are meant
/// to be used in async contexts.
///
/// # Examples
///
/// ```
/// # use kernel::prelude::*;
/// # use kernel::kasync::net::TcpStream;
/// async fn echo_server(stream: TcpStream) -> Result {
///     let mut buf = [0u8; 1024];
///     loop {
///         let n = stream.read(&mut buf).await?;
///         if n == 0 {
///             return Ok(());
///         }
///         stream.write_all(&buf[..n]).await?;
///     }
/// }
/// ```
pub struct TcpStream {
    stream: net::TcpStream,
}

impl TcpStream {
    /// Reads data from a connected socket.
    ///
    /// Returns a future that when ready indicates the result of the read operation; on success, it
    /// contains the number of bytes read, which will be zero if the connection is closed.
    pub fn read<'a>(&'a self, buf: &'a mut [u8]) -> impl Future<Output = Result<usize>> + 'a {
        SocketFuture::from_stream(
            self,
            bindings::BINDINGS_EPOLLIN | bindings::BINDINGS_EPOLLHUP | bindings::BINDINGS_EPOLLERR,
            || self.stream.read(buf, false),
        )
    }

    /// Writes data to the connected socket.
    ///
    /// Returns a future that when ready indicates the result of the write operation; on success, it
    /// contains the number of bytes written.
    pub fn write<'a>(&'a self, buf: &'a [u8]) -> impl Future<Output = Result<usize>> + 'a {
        SocketFuture::from_stream(
            self,
            bindings::BINDINGS_EPOLLOUT | bindings::BINDINGS_EPOLLHUP | bindings::BINDINGS_EPOLLERR,
            || self.stream.write(buf, false),
        )
    }

    /// Writes all the data to the connected socket.
    ///
    /// Returns a future that when ready indicates the result of the write operation; on success, it
    /// has written all the data.
    pub async fn write_all<'a>(&'a self, buf: &'a [u8]) -> Result {
        let mut rem = buf;

        while !rem.is_empty() {
            let n = self.write(rem).await?;
            rem = &rem[n..];
        }

        Ok(())
    }
}

impl Deref for TcpStream {
    type Target = net::TcpStream;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

/// A future for a socket operation.
///
/// # Invariants
///
/// `sock` is always non-null and valid for the duration of the lifetime of the instance.
struct SocketFuture<'a, Out, F: FnMut() -> Result<Out> + Send + 'a> {
    sock: *mut bindings::socket,
    mask: u32,
    is_queued: bool,
    wq_entry: Opaque<bindings::wait_queue_entry>,
    waker: NoWaitLock<Option<Waker>>,
    _p: PhantomData<&'a ()>,
    _pin: PhantomPinned,
    operation: F,
}

// SAFETY: A kernel socket can be used from any thread, `wq_entry` is only used on drop and when
// `is_queued` is initially `false`.
unsafe impl<Out, F: FnMut() -> Result<Out> + Send> Send for SocketFuture<'_, Out, F> {}

impl<'a, Out, F: FnMut() -> Result<Out> + Send + 'a> SocketFuture<'a, Out, F> {
    /// Creates a new socket future.
    ///
    /// # Safety
    ///
    /// Callers must ensure that `sock` is non-null, valid, and remains valid for the lifetime
    /// (`'a`) of the returned instance.
    unsafe fn new(sock: *mut bindings::socket, mask: u32, operation: F) -> Self {
        Self {
            sock,
            mask,
            is_queued: false,
            wq_entry: Opaque::uninit(),
            waker: NoWaitLock::new(None),
            operation,
            _p: PhantomData,
            _pin: PhantomPinned,
        }
    }

    /// Creates a new socket future for a tcp listener.
    fn from_listener(listener: &'a TcpListener, mask: u32, operation: F) -> Self {
        // SAFETY: The socket is guaranteed to remain valid because it is bound to the reference to
        // the listener (whose existence guarantees the socket remains valid).
        unsafe { Self::new(listener.listener.sock, mask, operation) }
    }

    /// Creates a new socket future for a tcp stream.
    fn from_stream(stream: &'a TcpStream, mask: u32, operation: F) -> Self {
        // SAFETY: The socket is guaranteed to remain valid because it is bound to the reference to
        // the stream (whose existence guarantees the socket remains valid).
        unsafe { Self::new(stream.stream.sock, mask, operation) }
    }

    /// Callback called when the socket changes state.
    ///
    /// If the state matches the one we're waiting on, we wake up the task so that the future can be
    /// polled again.
    #[klint::preempt_count(expect = 0..)] // This function can be called from `wake_up` which must noy sleep.
    unsafe extern "C" fn wake_callback(
        wq_entry: *mut bindings::wait_queue_entry,
        _mode: core::ffi::c_uint,
        _flags: core::ffi::c_int,
        key: *mut core::ffi::c_void,
    ) -> core::ffi::c_int {
        let mask = key as u32;

        // SAFETY: The future is valid while this callback is called because we remove from the
        // queue on drop.
        //
        // There is a potential soundness issue here because we're generating a shared reference to
        // `Self` while `Self::poll` has a mutable (unique) reference. However, for `!Unpin` types
        // (like `Self`), `&mut T` is treated as `*mut T` per
        // <https://github.com/rust-lang/rust/issues/63818> -- so we avoid the unsoundness. Once a
        // more definitive solution is available, we can change this to use it.
        let s = unsafe { &*crate::container_of!(wq_entry, Self, wq_entry) };
        if mask & s.mask == 0 {
            // Nothing to do as this notification doesn't interest us.
            return 0;
        }

        // If we can't acquire the waker lock, the waker is in the process of being modified. Our
        // attempt to acquire the lock will be reported to the lock owner, so it will trigger the
        // wake up.
        if let Some(guard) = s.waker.try_lock() {
            if let Some(ref w) = *guard {
                let cloned = w.clone();
                drop(guard);
                cloned.wake();
                return 1;
            }
        }
        0
    }

    /// Poll the future once.
    ///
    /// It calls the operation and converts `EAGAIN` errors into a pending state.
    fn poll_once(self: Pin<&mut Self>) -> Poll<Result<Out>> {
        // SAFETY: We never move out of `this`.
        let this = unsafe { self.get_unchecked_mut() };
        match (this.operation)() {
            Ok(s) => Poll::Ready(Ok(s)),
            Err(e) => {
                if e == EAGAIN {
                    Poll::Pending
                } else {
                    Poll::Ready(Err(e))
                }
            }
        }
    }

    /// Updates the waker stored in the future.
    ///
    /// It automatically triggers a wake up on races with the reactor.
    fn set_waker(&self, waker: &Waker) {
        if let Some(mut guard) = self.waker.try_lock() {
            let old = core::mem::replace(&mut *guard, Some(waker.clone()));
            let contention = guard.unlock();
            drop(old);
            if !contention {
                return;
            }
        }

        // We either couldn't store the waker because the existing one is being awakened, or the
        // reactor tried to acquire the lock while we held it (contention). In either case, we just
        // wake it up to ensure we don't miss any notification.
        waker.wake_by_ref();
    }
}

impl<Out, F: FnMut() -> Result<Out> + Send> Future for SocketFuture<'_, Out, F> {
    type Output = Result<Out>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().poll_once() {
            Poll::Ready(r) => Poll::Ready(r),
            Poll::Pending => {
                // Store away the latest waker every time we may `Pending`.
                self.set_waker(cx.waker());
                if self.is_queued {
                    // Nothing else to do was the waiter is already queued.
                    return Poll::Pending;
                }

                // SAFETY: We never move out of `this`.
                let this = unsafe { self.as_mut().get_unchecked_mut() };

                this.is_queued = true;

                // SAFETY: `wq_entry` is valid for write.
                unsafe {
                    bindings::init_waitqueue_func_entry(
                        this.wq_entry.get(),
                        Some(Self::wake_callback),
                    )
                };

                // SAFETY: `wq_entry` was just initialised above and is valid for read/write.
                // By the type invariants, the socket is always valid.
                unsafe {
                    bindings::add_wait_queue(
                        core::ptr::addr_of_mut!((*this.sock).wq.wait),
                        this.wq_entry.get(),
                    )
                };

                // If the future wasn't queued yet, we need to poll again in case it reached
                // the desired state between the last poll and being queued (in which case we
                // would have missed the notification).
                self.poll_once()
            }
        }
    }
}

impl<Out, F: FnMut() -> Result<Out> + Send> Drop for SocketFuture<'_, Out, F> {
    fn drop(&mut self) {
        if !self.is_queued {
            return;
        }

        // SAFETY: `wq_entry` is initialised because `is_queued` is set to `true`, so it is valid
        // for read/write. By the type invariants, the socket is always valid.
        unsafe {
            bindings::remove_wait_queue(
                core::ptr::addr_of_mut!((*self.sock).wq.wait),
                self.wq_entry.get(),
            )
        };
    }
}
