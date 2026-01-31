//! Convenience API for maintaining state, that notifies on changes.

mod once;
mod state;

pub use once::{Once, Stream as OnceStream};
pub use state::{State, Stream};
pub use zlink_core::notified as traits;
