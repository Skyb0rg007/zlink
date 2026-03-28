use crate::{
    Result,
    connection::socket::{self, Socket},
};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use tokio::net::{UnixStream, unix};

/// The connection type that uses Unix Domain Sockets for transport.
pub type Connection = crate::Connection<Stream>;

/// Connect to Unix Domain Socket at the given path.
pub async fn connect<P>(path: P) -> Result<Connection>
where
    P: AsRef<std::path::Path>,
{
    UnixStream::connect(path)
        .await
        .map(Stream)
        .map(Connection::new)
        .map_err(Into::into)
}

/// The [`Socket`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct Stream(UnixStream);

impl Socket for Stream {
    type ReadHalf = ReadHalf;
    type WriteHalf = WriteHalf;

    const CAN_TRANSFER_FDS: bool = true;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let (read, write) = self.0.into_split();

        (ReadHalf(read), WriteHalf(write))
    }
}

impl From<UnixStream> for Stream {
    fn from(stream: UnixStream) -> Self {
        Self(stream)
    }
}

impl socket::UnixSocket for Stream {}

impl AsFd for Stream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct ReadHalf(unix::OwnedReadHalf);

impl socket::ReadHalf for ReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<(usize, Vec<OwnedFd>)> {
        use std::{future::poll_fn, task::Poll};

        poll_fn(|cx| {
            loop {
                let stream: &UnixStream = self.0.as_ref();
                match stream.try_io(tokio::io::Interest::READABLE, || {
                    crate::unix_utils::recvmsg(stream, buf)
                }) {
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        match stream.poll_read_ready(cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(res) => res?,
                        }
                    }
                    v => return Poll::Ready(v.map_err(Into::into)),
                }
            }
        })
        .await
    }
}

impl AsFd for ReadHalf {
    fn as_fd(&self) -> BorrowedFd<'_> {
        let stream: &UnixStream = self.0.as_ref();
        stream.as_fd()
    }
}

impl socket::UnixSocket for ReadHalf {}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct WriteHalf(unix::OwnedWriteHalf);

impl socket::WriteHalf for WriteHalf {
    async fn write(&mut self, buf: &[u8], fds: &[impl AsFd]) -> Result<()> {
        use std::{future::poll_fn, task::Poll};

        // Convert to BorrowedFd for rustix.
        let borrowed_fds: Vec<BorrowedFd<'_>> = fds.iter().map(|f| f.as_fd()).collect();

        let mut pos = 0;
        while pos < buf.len() {
            // Use FDs on first write, empty slice on subsequent writes.
            let fds_to_send = if pos == 0 { &borrowed_fds[..] } else { &[] };

            let n: usize = poll_fn(|cx| {
                loop {
                    let stream: &UnixStream = self.0.as_ref();
                    match stream.try_io(tokio::io::Interest::WRITABLE, || {
                        crate::unix_utils::sendmsg(stream, &buf[pos..], fds_to_send)
                    }) {
                        Ok(bytes_sent) => return Poll::Ready(Ok::<_, crate::Error>(bytes_sent)),
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            match stream.poll_write_ready(cx) {
                                Poll::Pending => return Poll::Pending,
                                Poll::Ready(res) => res?,
                            }
                        }
                        Err(e) => return Poll::Ready(Err(e.into())),
                    }
                }
            })
            .await?;

            pos += n;
        }

        Ok(())
    }
}

impl AsFd for WriteHalf {
    fn as_fd(&self) -> BorrowedFd<'_> {
        let stream: &UnixStream = self.0.as_ref();
        stream.as_fd()
    }
}

impl socket::UnixSocket for WriteHalf {}
