use core::future::Future;

use crate::{Connection, Result, connection::Socket};

/// A listener is a server that listens for incoming connections.
pub trait Listener: core::fmt::Debug {
    /// The type of the socket the connections this listener creates will use.
    type Socket: Socket;

    /// Accept a new connection.
    ///
    /// Returns `Ok(None)` to signal that no more connections will be produced. Once `Ok(None)`
    /// has been returned, subsequent calls must pend forever — the listener is considered closed.
    fn accept(&mut self) -> impl Future<Output = Result<Option<Connection<Self::Socket>>>>;
}

/// A listener that already has a socket.
///
/// This is useful for services that get spawned by systemd and handed over a socket.
#[derive(Debug)]
pub struct ReadyListener<Sock: Socket> {
    socket: Option<Sock>,
}

impl<Sock> ReadyListener<Sock>
where
    Sock: Socket,
{
    /// Create a new listener from a socket.
    pub fn new(socket: Sock) -> Self {
        Self {
            socket: Some(socket),
        }
    }
}

impl<Sock> Listener for ReadyListener<Sock>
where
    Sock: Socket,
{
    type Socket = Sock;

    /// This implementation simply returns the contained socket.
    ///
    /// After the first call, in simply never returns on subsequent calls.
    async fn accept(&mut self) -> Result<Option<Connection<Self::Socket>>> {
        match self.socket.take() {
            Some(socket) => Ok(Some(Connection::new(socket))),
            None => core::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::{future::poll_fn, task::Poll};

    use super::*;
    use crate::test_utils::mock_socket::MockSocket;

    #[tokio::test]
    async fn ready_listener() {
        let socket = MockSocket::with_responses(&["test"]);
        let mut listener = ReadyListener::new(socket);

        // First call returns a connection with properly split read/write halves.
        let conn = listener.accept().await.unwrap().unwrap();
        let (read, write) = conn.split();
        assert_eq!(read.id(), write.id());

        // Second call should be pending forever.
        let accept_fut = listener.accept();
        futures_util::pin_mut!(accept_fut);
        let is_pending = poll_fn(|cx| Poll::Ready(accept_fut.as_mut().poll(cx).is_pending())).await;
        assert!(is_pending);
    }
}
