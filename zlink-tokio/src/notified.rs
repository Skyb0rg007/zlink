//! Convenience API for maintaining state, that notifies on changes.

use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Reply;
use pin_project_lite::pin_project;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::BroadcastStream;

/// A notified state (e.g a field) of a service implementation.
#[derive(Debug, Clone)]
pub struct State<T, ReplyParams> {
    value: T,
    tx: broadcast::Sender<ReplyParams>,
}

impl<T, ReplyParams> State<T, ReplyParams>
where
    T: Into<ReplyParams> + Clone + Debug,
    ReplyParams: Clone + Send + 'static + Debug,
{
    /// Create a new notified field.
    pub fn new(value: T) -> Self {
        let (tx, _) = broadcast::channel(1);

        Self { value, tx }
    }

    /// Set the value of the notified field and notify all listeners.
    pub async fn set(&mut self, value: T) {
        self.value = value.clone();
        // Failure means that there are currently no receivers and that's ok.
        let _ = self.tx.send(value.into());
    }

    /// Get the value of the notified field.
    pub fn get(&self) -> T {
        self.value.clone()
    }

    /// Get a stream of replies for the notified field.
    pub fn stream(&self) -> Stream<ReplyParams> {
        Stream {
            inner: StreamInner::Broadcast {
                stream: self.tx.subscribe().into(),
                cached: None,
            },
        }
    }
}

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

        (
            Self { tx },
            Stream {
                inner: StreamInner::Oneshot { receiver: rx },
            },
        )
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
    /// using [`State`] or [`Once`].
    #[derive(Debug)]
    pub struct Stream<ReplyParams> {
        #[pin]
        inner: StreamInner<ReplyParams>,
    }
}

impl<ReplyParams> futures_util::Stream for Stream<ReplyParams>
where
    ReplyParams: Clone + Send + 'static,
{
    type Item = Reply<ReplyParams>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.inner.project() {
            StreamInnerProj::Broadcast { mut stream, cached } => loop {
                match futures_util::ready!(stream.as_mut().poll_next(cx)) {
                    Some(Ok(reply)) => {
                        // Cache and yield immediately with continues=true.
                        *cached = Some(reply.clone());
                        break Poll::Ready(Some(Reply::new(Some(reply)).set_continues(Some(true))));
                    }
                    // Some intermediate values were missed. That's OK, as long as we get the
                    // latest value.
                    Some(Err(_)) => continue,
                    // Channel closed - yield cached value with continues=false.
                    None => {
                        break Poll::Ready(
                            cached
                                .take()
                                .map(|reply| Reply::new(Some(reply)).set_continues(Some(false))),
                        )
                    }
                }
            },
            StreamInnerProj::Oneshot { receiver } => {
                if receiver.is_terminated() {
                    return Poll::Ready(None);
                }

                receiver.poll(cx).map(|reply| {
                    reply
                        .map(|reply| Reply::new(Some(reply)).set_continues(Some(false)))
                        .ok()
                })
            }
        }
    }
}

pin_project! {
    #[project = StreamInnerProj]
    #[derive(Debug)]
    enum StreamInner<ReplyParams> {
        Broadcast {
            #[pin]
            stream: BroadcastStream<ReplyParams>,
            cached: Option<ReplyParams>,
        },
        Oneshot {
            #[pin]
            receiver: oneshot::Receiver<ReplyParams>,
        },
    }
}
