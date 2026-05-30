//! Connection credentials.

use super::{Gid, Pid, Uid};

/// Credentials of a peer connection.
#[derive(Debug)]
pub struct Credentials {
    basic: PassedCredentials,
    #[cfg(target_os = "linux")]
    unix_supplementary_group_ids: Vec<Gid>,
    #[cfg(target_os = "linux")]
    process_fd: std::os::fd::OwnedFd,
}

impl Credentials {
    /// Create new credentials for a peer connection.
    pub(crate) fn new(
        basic: PassedCredentials,
        #[cfg(target_os = "linux")] unix_supplementary_group_ids: Vec<Gid>,
        #[cfg(target_os = "linux")] process_fd: std::os::fd::OwnedFd,
    ) -> Self {
        Self {
            basic,
            #[cfg(target_os = "linux")]
            unix_supplementary_group_ids,
            #[cfg(target_os = "linux")]
            process_fd,
        }
    }

    /// The numeric Unix user ID, as defined by POSIX.
    pub fn unix_user_id(&self) -> Uid {
        self.basic.unix_user_id
    }

    /// The numeric process ID, on platforms that have this concept.
    ///
    /// On Unix, this is the process ID defined by POSIX.
    pub fn process_id(&self) -> Pid {
        self.basic.process_id
    }

    /// The numeric Unix group ID, as defined by POSIX.
    pub fn unix_primary_group_id(&self) -> Gid {
        self.basic.unix_primary_group_id
    }

    /// The set of numeric supplementary Unix group IDs, as defined by POSIX.
    ///
    /// Currently, this method is only available for Linux targets.
    #[cfg(target_os = "linux")]
    pub fn unix_supplementary_group_ids(&self) -> &[Gid] {
        &self.unix_supplementary_group_ids
    }

    /// A file descriptor pinning the process, on platforms that have this concept.
    ///
    /// On Linux, the SO_PEERPIDFD socket option is a suitable implementation. This is safer to use
    /// to identify a process than the ProcessID, as the latter is subject to re-use attacks, while
    /// the FD cannot be recycled. If the original process no longer exists the FD will no longer
    /// be resolvable.
    #[cfg(target_os = "linux")]
    pub fn process_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;

        self.process_fd.as_fd()
    }
}

/// Credentials passed over of socket.
#[derive(Debug)]
pub struct PassedCredentials {
    unix_user_id: Uid,
    unix_primary_group_id: Gid,
    process_id: Pid,
}

impl PassedCredentials {
    pub(crate) fn new(unix_user_id: Uid, unix_primary_group_id: Gid, process_id: Pid) -> Self {
        Self {
            unix_user_id,
            unix_primary_group_id,
            process_id,
        }
    }

    /// The numeric Unix user ID, as defined by POSIX.
    pub fn unix_user_id(&self) -> Uid {
        self.unix_user_id
    }

    /// The numeric process ID, on platforms that have this concept.
    ///
    /// On Unix, this is the process ID defined by POSIX.
    pub fn process_id(&self) -> Pid {
        self.process_id
    }

    /// The numeric Unix group ID, as defined by POSIX.
    pub fn unix_primary_group_id(&self) -> Gid {
        self.unix_primary_group_id
    }
}
