//! Provides transport over Unix Domain Sockets.

mod stream;
pub use stream::{Connection, Stream, connect};
#[cfg(feature = "server")]
mod listener;
#[cfg(feature = "server")]
pub use listener::{Listener, bind};
