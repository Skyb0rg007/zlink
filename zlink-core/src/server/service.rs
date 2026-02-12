//! Service-related API.

use core::{fmt::Debug, future::Future};

use futures_util::Stream;
use serde::{Deserialize, Serialize};

use crate::{connection::Socket, Call, Connection, Reply};

/// The item type that a [`Service::ReplyStream`] yields.
///
/// On `std`, this is a tuple of the reply and the file descriptors to send with it. On `no_std`,
/// this is just the reply.
#[cfg(feature = "std")]
pub type ReplyStreamItem<Params> = (Reply<Params>, Vec<std::os::fd::OwnedFd>);
/// The item type that a [`Service::ReplyStream`] yields.
///
/// On `std`, this is a tuple of the reply and the file descriptors to send with it. On `no_std`,
/// this is just the reply.
#[cfg(not(feature = "std"))]
pub type ReplyStreamItem<Params> = Reply<Params>;

/// Service trait for handling method calls.
///
/// Instead of implementing this trait manually, prefer using the [`service`] attribute macro which
/// generates the implementation for you. The macro provides a more ergonomic API and handles the
/// boilerplate of method dispatching, error handling, and streaming replies.
///
/// See the [`service`] macro documentation for details and examples.
///
/// [`service`]: macro@crate::service
pub trait Service<Sock>
where
    Sock: Socket,
{
    /// The type of method call that this service handles.
    ///
    /// This should be a type that can deserialize itself from a complete method call message: i-e
    /// an object containing `method` and `parameter` fields. This can be easily achieved using the
    /// `serde::Deserialize` derive (See the code snippet in
    /// [`crate::connection::WriteConnection::send_call`] documentation for an example).
    type MethodCall<'de>: Deserialize<'de> + Debug;
    /// The type of the successful reply.
    ///
    /// This should be a type that can serialize itself as the `parameters` field of the reply.
    type ReplyParams<'ser>: Serialize + Debug
    where
        Self: 'ser;
    /// The type of the item that [`Service::ReplyStream`] will be expected to yield.
    ///
    /// This should be a type that can serialize itself as the `parameters` field of the reply.
    type ReplyStreamParams: Serialize + Debug;
    /// The type of the multi-reply stream.
    ///
    /// If the client asks for multiple replies, this stream will be used to send them.
    type ReplyStream: Stream<Item = ReplyStreamItem<Self::ReplyStreamParams>> + Unpin;
    /// The type of the error reply.
    ///
    /// This should be a type that can serialize itself to the whole reply object, containing
    /// `error` and `parameter` fields. This can be easily achieved using the `serde::Serialize`
    /// derive (See the code snippet in [`crate::connection::ReadConnection::receive_reply`]
    /// documentation for an example).
    type ReplyError<'ser>: Serialize + Debug
    where
        Self: 'ser;

    /// Handle a method call.
    fn handle<'ser>(
        &'ser mut self,
        method: &'ser Call<Self::MethodCall<'_>>,
        conn: &mut Connection<Sock>,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> impl Future<
        Output = HandleResult<Self::ReplyParams<'ser>, Self::ReplyStream, Self::ReplyError<'ser>>,
    >;
}

/// The result of a [`Service::handle`] call.
///
/// On `std`, this is a tuple of the method reply and the file descriptors to send with it. On
/// `no_std`, this is just the method reply.
#[cfg(feature = "std")]
pub type HandleResult<Params, ReplyStream, ReplyError> = (
    MethodReply<Params, ReplyStream, ReplyError>,
    Vec<std::os::fd::OwnedFd>,
);
/// The result of a [`Service::handle`] call.
///
/// On `std`, this is a tuple of the method reply and the file descriptors to send with it. On
/// `no_std`, this is just the method reply.
#[cfg(not(feature = "std"))]
pub type HandleResult<Params, ReplyStream, ReplyError> =
    MethodReply<Params, ReplyStream, ReplyError>;

/// A service method call reply.
#[derive(Debug)]
pub enum MethodReply<Params, ReplyStream, ReplyError> {
    /// A single reply.
    Single(Option<Params>),
    /// An error reply.
    Error(ReplyError),
    /// A multi-reply stream.
    Multi(ReplyStream),
}
