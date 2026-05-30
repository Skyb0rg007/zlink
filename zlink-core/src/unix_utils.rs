//! Helper functions for Unix socket FD passing.
//!
//! These are public but hidden from documentation as they're implementation details shared between
//! runtime-specific socket implementations.

use core::mem::MaybeUninit;
#[cfg(target_os = "linux")]
use std::os::fd::OwnedFd;
use std::{
    io,
    os::fd::{AsFd, BorrowedFd},
};

use crate::connection::{Credentials, PassedCredentials, socket::ReadResult};

/// Receive a message from a Unix socket, including any file descriptors.
///
/// This is a low-level helper that performs the `recvmsg` syscall.
#[doc(hidden)]
pub fn recvmsg(fd: impl AsFd, buf: &mut [u8]) -> io::Result<ReadResult> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};
    use std::io::IoSliceMut;

    let mut cmsg_buf = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut cmsg_buf);

    let mut iov = [IoSliceMut::new(buf)];
    recvmsg(fd.as_fd(), &mut iov, &mut control, RecvFlags::empty())
        .map(|msg| {
            // Extract file descriptors from ancillary data.
            let mut fds = alloc::vec::Vec::new();
            for m in control.drain() {
                if let RecvAncillaryMessage::ScmRights(rights) = m {
                    fds.extend(rights);
                }
            }
            let result = ReadResult::new(msg.bytes);
            #[cfg(feature = "std")]
            let result = result.set_fds(fds);

            result
        })
        .map_err(io::Error::from)
}

/// Send a message to a Unix socket, including any file descriptors.
///
/// This is a low-level helper that performs the `sendmsg` syscall.
#[doc(hidden)]
pub fn sendmsg(fd: impl AsFd, buf: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<usize> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::io::IoSlice;

    let mut cmsg_buf = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut cmsg_buf);

    if !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many file descriptors to send",
        ));
    }

    let iov = [IoSlice::new(buf)];
    sendmsg(fd.as_fd(), &iov, &mut control, SendFlags::empty()).map_err(io::Error::from)
}

/// Get the peer credentials from a Unix socket.
///
/// This is a low-level helper that fetches credentials using platform-specific APIs.
///
/// # Platform Support
///
/// - **Linux/Android**: Uses `SO_PEERCRED` to get uid and pid. On Linux, also gets `SO_PEERPIDFD`
///   for process FD (falls back to `pidfd_open` if not available).
/// - **macOS/iOS**: Uses `getpeereid()` for uid and `LOCAL_PEERPID` for pid.
/// - **OpenBSD**: Uses `getpeereid()` for uid and `SO_PEERCRED` for pid.
/// - **NetBSD**: Uses `getpeereid()` for uid and `LOCAL_PEEREID` for pid.
/// - **FreeBSD/DragonFly**: Uses `getpeereid()` for uid. PID is 0 (FIXME: use `LOCAL_PEERCRED`).
pub(crate) fn get_peer_credentials(fd: impl AsFd) -> io::Result<Credentials> {
    use std::os::fd::AsRawFd;

    let fd = fd.as_fd();

    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        use std::os::fd::FromRawFd;

        // Get SO_PEERCRED (uid, gid, pid).
        let ucred = rustix::net::sockopt::socket_peercred(fd)?;
        let uid = ucred.uid;
        let pid = ucred.pid;
        let primary_gid = ucred.gid;

        // Get SO_PEERGROUPS if available (Linux-only).
        #[cfg(target_os = "linux")]
        let supplementary_gids = {
            use rustix::fs::Gid;

            let mut nr_supp_gids = INITIAL_NUMBER_SUPPLEMENTARY_GROUPS;
            let mut nr_supp_gids_in_bytes = nr_supp_gids * (size_of::<Gid>() as u32);
            let mut supp_gids: Vec<Gid> = Vec::with_capacity(nr_supp_gids as usize);

            loop {
                let ret = unsafe {
                    libc::getsockopt(
                        fd.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_PEERGROUPS,
                        supp_gids.as_mut_ptr().cast(),
                        &mut nr_supp_gids_in_bytes,
                    )
                };
                let err = io::Error::last_os_error();

                // We encountered an error which is not about the size of our passed container.
                if ret == -1 && err.raw_os_error() != Some(libc::ERANGE) {
                    return Err(err);
                }

                // If the number of groups returned is less than the requested size, we are done.
                nr_supp_gids = nr_supp_gids_in_bytes / size_of::<Gid>() as u32;
                if nr_supp_gids as usize <= supp_gids.capacity() {
                    supp_gids.shrink_to(nr_supp_gids as usize);
                    // SAFETY: `getsockopt` filled at least `nr_supp_gids` items in the buffer.
                    unsafe { supp_gids.set_len(nr_supp_gids as usize) };
                    break;
                }

                // Otherwise, the vector is too small. Resize and try again.
                // We let the standard Vector speculation over-allocation take place here on
                // purpose.
                supp_gids.reserve(nr_supp_gids as usize - supp_gids.capacity());
                // SAFETY: The number of supplementary GIDs on Linux is bounded 65k which fits in
                // u32.
                nr_supp_gids_in_bytes = (supp_gids.capacity() as u32) * (size_of::<Gid>() as u32);
            }

            supp_gids
        };

        // Get SO_PEERPIDFD if available (Linux-only).
        #[cfg(target_os = "linux")]
        let process_fd = {
            // FIXME: Replace `libc` usage with `rustix` API when it provides SO_PEERPIDFD
            // sockopt: https://github.com/bytecodealliance/rustix/pull/1474
            use core::mem::{MaybeUninit, size_of};

            let mut pidfd = MaybeUninit::<libc::c_int>::zeroed();
            let mut len = size_of::<libc::c_int>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERPIDFD,
                    pidfd.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            // `getsockopt` returns `0` on success or `-1` on error.
            if ret == 0 {
                let pidfd = unsafe { pidfd.assume_init() };
                unsafe { OwnedFd::from_raw_fd(pidfd) }
            } else {
                let err = io::Error::last_os_error();
                // ENOPROTOOPT means the kernel doesn't support this feature.
                if err.raw_os_error() != Some(libc::ENOPROTOOPT) {
                    return Err(err);
                }
                // If SO_PEERPIDFD is not supported, we fall back to using pidfd_open.
                rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())?
            }
        };

        #[cfg(target_os = "android")]
        let creds = Credentials::new(PassedCredentials::new(uid, primary_gid, pid));
        #[cfg(target_os = "linux")]
        let creds = Credentials::new(
            PassedCredentials::new(uid, primary_gid, pid),
            supplementary_gids,
            process_fd,
        );

        Ok(creds)
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        // FIXME: Replace with rustix API when it provides the required API:
        // https://github.com/bytecodealliance/rustix/issues/1533
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;

        let ret = unsafe { libc::getpeereid(fd.as_raw_fd(), &mut uid, &mut gid) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }

        let uid = rustix::process::Uid::from_raw(uid);
        let gid = rustix::process::Gid::from_raw(gid);

        // Platform-specific PID fetching.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let pid = {
            let mut pid: libc::pid_t = 0;
            let mut len = core::mem::size_of::<libc::pid_t>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_LOCAL,
                    libc::LOCAL_PEERPID,
                    (&raw mut pid).cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            rustix::process::Pid::from_raw(pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        #[cfg(target_os = "openbsd")]
        let pid = {
            // OpenBSD's SO_PEERCRED returns struct sockpeercred { uid, gid, pid }.
            #[repr(C)]
            struct sockpeercred {
                uid: libc::uid_t,
                gid: libc::gid_t,
                pid: libc::pid_t,
            }

            let mut creds = core::mem::MaybeUninit::<sockpeercred>::zeroed();
            let mut len = core::mem::size_of::<sockpeercred>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    creds.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            let creds = unsafe { creds.assume_init() };
            rustix::process::Pid::from_raw(creds.pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        #[cfg(target_os = "netbsd")]
        let pid = {
            // NetBSD's LOCAL_PEEREID returns struct unpcbid { pid, euid, egid }.
            #[repr(C)]
            struct unpcbid {
                unp_pid: libc::pid_t,
                unp_euid: libc::uid_t,
                unp_egid: libc::gid_t,
            }

            const LOCAL_PEEREID: libc::c_int = 3;

            let mut creds = core::mem::MaybeUninit::<unpcbid>::zeroed();
            let mut len = core::mem::size_of::<unpcbid>() as libc::socklen_t;

            let ret = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    0, // SOL_LOCAL
                    LOCAL_PEEREID,
                    creds.as_mut_ptr().cast(),
                    &mut len,
                )
            };

            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            let creds = unsafe { creds.assume_init() };
            rustix::process::Pid::from_raw(creds.unp_pid)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid peer PID"))?
        };

        // FIXME: FreeBSD 13+ has cr_pid in xucred, DragonFly status unknown.
        #[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
        let pid = rustix::process::Pid::from_raw(0).unwrap();

        Ok(Credentials::new(PassedCredentials::new(uid, gid, pid)))
    }

    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credentials not supported on this platform",
        ))
    }
}

/// The maximum number of file descriptors that can be sent in a single message.
///
/// The value is based on what is used in `zbus`, which comes from sdbus.
const MAX_FDS: usize = 1024;

// Linux can go up to NGROUPS_MAX supplementary groups (65K). It is safe to assume that
// most users will have a couple of supplementary groups by default. We allocate 128
// because integers are tiny.
#[cfg(target_os = "linux")]
const INITIAL_NUMBER_SUPPLEMENTARY_GROUPS: libc::socklen_t = 128 as libc::socklen_t;
