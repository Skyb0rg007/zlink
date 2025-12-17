use core::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};
use futures_util::stream::Stream;
use pin_project_lite::pin_project;
use serde::de::DeserializeOwned;

use crate::{
    connection::{socket::ReadHalf, ReadConnection},
    reply, Result,
};

#[cfg(feature = "std")]
use std::os::fd::OwnedFd;

/// Type alias for chain reply results.
///
/// In std mode, includes file descriptors received with the reply.
/// In no_std mode, just the reply result.
#[cfg(feature = "std")]
pub(crate) type ChainResult<Params, ReplyError> =
    (reply::Result<Params, ReplyError>, alloc::vec::Vec<OwnedFd>);

#[cfg(not(feature = "std"))]
pub(crate) type ChainResult<Params, ReplyError> = reply::Result<Params, ReplyError>;

pin_project! {
    /// A stream of replies from a chain of method calls.
    ///
    /// # Owned Data Requirement
    ///
    /// Stream items must use owned types (`DeserializeOwned`) rather than borrowed types. This is
    /// because the internal buffer may be reused between stream iterations, which would invalidate
    /// borrowed references. This limitation may be lifted in the future when Rust supports lending
    /// streams.
    #[derive(Debug)]
    pub struct ReplyStream<'c, Read: ReadHalf, F, Fut, Params, ReplyError> {
        #[pin]
        state: ReplyStreamState<Fut>,
        connection: &'c mut ReadConnection<Read>,
        func: F,
        call_count: usize,
        current_index: usize,
        done: bool,
        _phantom: core::marker::PhantomData<(Params, ReplyError)>,
    }
}

impl<'c, Read, F, Fut, Params, ReplyError> ReplyStream<'c, Read, F, Fut, Params, ReplyError>
where
    Read: ReadHalf,
    F: FnMut(&'c mut ReadConnection<Read>) -> Fut,
    Fut: Future<Output = Result<ChainResult<Params, ReplyError>>>,
    Params: DeserializeOwned + Debug,
    ReplyError: DeserializeOwned + Debug,
{
    /// Create a new reply stream.
    ///
    /// This is used internally by the proxy macro for streaming methods.
    #[doc(hidden)]
    pub fn new(connection: &'c mut ReadConnection<Read>, func: F, call_count: usize) -> Self {
        ReplyStream {
            state: ReplyStreamState::Init,
            connection,
            func,
            call_count,
            current_index: 0,
            done: false,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<'c, Read, F, Fut, Params, ReplyError> Stream
    for ReplyStream<'c, Read, F, Fut, Params, ReplyError>
where
    Read: ReadHalf,
    F: FnMut(&'c mut ReadConnection<Read>) -> Fut,
    Fut: Future<Output = Result<ChainResult<Params, ReplyError>>>,
    Params: DeserializeOwned + Debug,
    ReplyError: DeserializeOwned + Debug,
{
    type Item = Result<ChainResult<Params, ReplyError>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if *this.done {
            return Poll::Ready(None);
        }

        if this.state.as_mut().check_init() {
            let conn = unsafe { &mut *(*this.connection as *mut _) };
            this.state.set(ReplyStreamState::Future {
                future: (this.func)(conn),
            });
        }

        let item = match this.state.as_mut().project_future() {
            Some(fut) => ready!(fut.poll(cx)),
            None => panic!("ReplyStream must not be polled after it returned `Poll::Ready(None)`"),
        };

        // Only increment current_index if this is the last reply for this call.
        // (i.e., continues is not Some(true))
        #[cfg(feature = "std")]
        {
            match &item {
                Ok((Ok(reply), _fds)) if reply.continues() != Some(true) => {
                    *this.current_index += 1;
                }
                Ok((Ok(_), _fds)) => {
                    // Streaming reply, don't increment index yet.
                }
                Ok((Err(_), _fds)) => {
                    // For method errors, always increment since there won't be more replies.
                    *this.current_index += 1;
                }
                Err(_) => {
                    // If there was a general error, mark the stream as done as it's likely not
                    // recoverable.
                    *this.done = true;
                }
            }
        }
        #[cfg(not(feature = "std"))]
        {
            match &item {
                Ok(Ok(reply)) if reply.continues() != Some(true) => {
                    *this.current_index += 1;
                }
                Ok(Ok(_)) => {
                    // Streaming reply, don't increment index yet.
                }
                Ok(Err(_)) => {
                    // For method errors, always increment since there won't be more replies.
                    *this.current_index += 1;
                }
                Err(_) => {
                    // If there was a general error, mark the stream as done as it's likely not
                    // recoverable.
                    *this.done = true;
                }
            }
        }
        if *this.current_index >= *this.call_count {
            *this.done = true;
        }

        this.state.set(ReplyStreamState::Init);

        Poll::Ready(Some(item))
    }
}

pin_project! {
    /// State for `ReplyStream`.
    ///
    /// Based on the [`futures::stream::unfold`] implementation.
    #[project = ReplyStreamStateProj]
    #[project_replace = ReplyStreamStateProjReplace]
    #[derive(Debug)]
    enum ReplyStreamState<R> {
        Init,
        Future {
            #[pin]
            future: R,
        },
        Empty,
    }
}

impl<R> ReplyStreamState<R> {
    fn project_future(self: Pin<&mut Self>) -> Option<Pin<&mut R>> {
        match self.project() {
            ReplyStreamStateProj::Future { future } => Some(future),
            _ => None,
        }
    }

    fn check_init(self: Pin<&mut Self>) -> bool {
        match &*self {
            Self::Init => match self.project_replace(Self::Empty) {
                ReplyStreamStateProjReplace::Init => true,
                _ => unreachable!(),
            },
            _ => false,
        }
    }
}
