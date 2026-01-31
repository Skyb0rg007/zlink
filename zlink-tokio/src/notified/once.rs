use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Reply;
use pin_project_lite::pin_project;
use tokio::sync::oneshot;

/// A one-shot notified state of a service implementation.
///
/// This is useful for handling method calls in a separate task/thread.
#[derive(Debug)]
pub struct Once<ReplyParams> {
    tx: oneshot::Sender<ReplyParams>,
}

impl<ReplyParams> Once<ReplyParams>
where
    ReplyParams: Send + 'static + Debug,
{
    /// Create a new notified oneshot state.
    pub fn new() -> (Self, Stream<ReplyParams>) {
        let (tx, rx) = oneshot::channel();

        (Self { tx }, Stream { inner: rx })
    }

    /// Set the value of the notified field and notify all listeners.
    pub fn notify<T>(self, value: T)
    where
        T: Into<ReplyParams> + Debug,
    {
        // Failure means that we dropped the receiver stream internally before it received anything
        // and that's a big bug that must not happen.
        self.tx.send(value.into()).unwrap();
    }
}

pin_project! {
    /// The stream to use as the [`crate::Service::ReplyStream`] in service implementation when
    /// using [`Once`].
    #[derive(Debug)]
    pub struct Stream<ReplyParams> {
        #[pin]
        inner: oneshot::Receiver<ReplyParams>,
    }
}

impl<ReplyParams> futures_util::Stream for Stream<ReplyParams>
where
    ReplyParams: Send + 'static,
{
    type Item = Reply<ReplyParams>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if this.inner.is_terminated() {
            return Poll::Ready(None);
        }

        this.inner.poll(cx).map(|reply| {
            reply
                .map(|reply| Reply::new(Some(reply)).set_continues(Some(false)))
                .ok()
        })
    }
}
