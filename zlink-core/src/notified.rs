//! Notified state API traits.
//!
//! This module defines traits that document the expected API for notified types. Runtime crates
//! (like `zlink-tokio` and `zlink-smol`) implement these traits on their concrete types.

use core::{fmt::Debug, future::Future};

use crate::Reply;

/// Trait for a notified state that tracks a value and broadcasts changes.
///
/// This is useful for implementing service properties that notify subscribers when they change.
pub trait State<T, ReplyParams>: Clone
where
    T: Into<ReplyParams> + Clone + Debug,
    ReplyParams: Clone + Send + 'static + Debug,
{
    /// The stream type returned by [`stream`](Self::stream).
    type Stream: futures_util::Stream<Item = Reply<ReplyParams>>;

    /// Create a new notified state with the given initial value.
    fn new(value: T) -> Self;

    /// Set the value and notify all listeners.
    fn set(&mut self, value: T) -> impl Future<Output = ()> + Send;

    /// Get the current value.
    fn get(&self) -> T;

    /// Get a stream of replies for this state.
    fn stream(&self) -> Self::Stream;

    /// Get a stream of replies for this state, that only yields one reply: the current state.
    fn stream_once(&self) -> Self::Stream;
}
