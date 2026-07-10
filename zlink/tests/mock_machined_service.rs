//! Mock systemd-machined service for testing when real systemd services aren't available.

#![cfg(all(feature = "service", feature = "introspection", feature = "idl-parse"))]

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use zlink::{
    ReplyError,
    introspect::{self, CustomType, Type},
};

// ============================================================================
// Shared types (needed by both client proxy and server mock)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ListReply<'a> {
    pub name: &'a str,
    pub id: Option<&'a str>,
    pub service: Option<&'a str>,
    pub class: &'a str,
    pub leader: Option<ProcessId<'a>>,
    #[serde(rename = "rootDirectory")]
    pub root_directory: Option<&'a str>,
    // Needs owned variant for deserializing because of escaped content.
    pub unit: Option<Cow<'a, str>>,
    pub timestamp: Option<Timestamp>,
    #[serde(rename = "vSockCid")]
    pub v_sock_cid: Option<u64>,
    #[serde(rename = "sshAddress")]
    pub ssh_address: Option<&'a str>,
    #[serde(rename = "sshPrivateKeyPath")]
    pub ssh_private_key_path: Option<&'a str>,
    pub addresses: Option<Vec<Address>>,
    #[serde(rename = "OSRelease")]
    pub os_release: Option<Vec<&'a str>>,
    #[serde(rename = "UIDShift")]
    pub uid_shift: Option<u64>,
}

// Owned version for streaming APIs (which require DeserializeOwned).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedListReply {
    pub name: String,
    pub id: Option<String>,
    pub service: Option<String>,
    pub class: String,
    pub leader: Option<OwnedProcessId>,
    #[serde(rename = "rootDirectory")]
    pub root_directory: Option<String>,
    pub unit: Option<String>,
    pub timestamp: Option<Timestamp>,
    #[serde(rename = "vSockCid")]
    pub v_sock_cid: Option<u64>,
    #[serde(rename = "sshAddress")]
    pub ssh_address: Option<String>,
    #[serde(rename = "sshPrivateKeyPath")]
    pub ssh_private_key_path: Option<String>,
    pub addresses: Option<Vec<Address>>,
    #[serde(rename = "OSRelease")]
    pub os_release: Option<Vec<String>>,
    #[serde(rename = "UIDShift")]
    pub uid_shift: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
#[serde(rename_all = "lowercase")]
pub enum AcquireMetadata {
    No,
    Yes,
    Graceful,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
#[serde(rename_all = "lowercase")]
pub enum MachineOpenMode {
    Tty,
    Login,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct ProcessId<'a> {
    pub pid: i64,
    #[serde(rename = "pidfdId")]
    pub pidfd_id: Option<u64>,
    #[serde(rename = "bootId")]
    // According to the IDL, this should be a number but we actually get a string.
    // See https://github.com/systemd/systemd/issues/38276
    pub boot_id: Option<&'a str>,
}

// Owned version for streaming APIs (which require DeserializeOwned).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct OwnedProcessId {
    pub pid: i64,
    #[serde(rename = "pidfdId")]
    pub pidfd_id: Option<u64>,
    #[serde(rename = "bootId")]
    pub boot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct Timestamp {
    pub realtime: Option<u64>,
    pub monotonic: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, CustomType)]
pub struct Address {
    pub ifindex: Option<u64>,
    pub family: i64,
    pub address: Vec<u64>,
}

/// Errors that can be returned by the `io.systemd.Machine` interface.
#[derive(Debug, Clone, PartialEq, ReplyError, introspect::ReplyError)]
#[zlink(interface = "io.systemd.Machine")]
pub enum MachinedError {
    /// No matching machine currently running.
    NoSuchMachine,
    /// Machine already exists.
    MachineExists,
    /// Machine does not use private networking.
    NoPrivateNetworking,
    /// Machine does not contain OS release information.
    NoOSReleaseInformation,
    /// Machine uses a complex UID/GID mapping, cannot determine shift.
    NoUIDShift,
    /// Requested information is not available.
    NotAvailable,
    /// Requested operation is not supported.
    NotSupported,
    /// There is no IPC service (such as system bus or varlink) in the container.
    NoIPC,
    /// Failed to fetch peer credentials.
    FetchPeerCredentialsFailed,
}

impl core::fmt::Display for MachinedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MachinedError::NoSuchMachine => write!(f, "No such machine"),
            MachinedError::MachineExists => write!(f, "Machine already exists"),
            MachinedError::NoPrivateNetworking => {
                write!(f, "Machine does not use private networking")
            }
            MachinedError::NoOSReleaseInformation => {
                write!(f, "Machine does not contain OS release information")
            }
            MachinedError::NoUIDShift => write!(
                f,
                "Machine uses a complex UID/GID mapping, cannot determine shift"
            ),
            MachinedError::NotAvailable => write!(f, "Requested information is not available"),
            MachinedError::NotSupported => write!(f, "Requested operation is not supported"),
            MachinedError::NoIPC => write!(f, "There is no IPC service in the container"),
            MachinedError::FetchPeerCredentialsFailed => {
                write!(f, "Failed to fetch peer credentials")
            }
        }
    }
}

impl core::error::Error for MachinedError {}

// ============================================================================
// Server-only types and implementation (mock service)
// ============================================================================

#[cfg(feature = "server")]
#[path = "creds-utils.rs"]
mod creds_utils;

#[cfg(feature = "server")]
mod server {
    use super::*;
    use zlink::connection::socket::FetchPeerCredentials;

    /// Owned version of ProcessId for use in service method parameters.
    /// The service macro clones parameters, so we need owned types.
    /// Named `ProcessId` to match the IDL output of the real machined service.
    #[allow(dead_code)]
    #[derive(Debug, Clone, Deserialize, CustomType)]
    pub struct ProcessId {
        pub pid: i64,
        #[serde(rename = "pidfdId")]
        pub pidfd_id: Option<u64>,
        #[serde(rename = "bootId")]
        pub boot_id: Option<String>,
    }

    /// Reply for Open method.
    #[derive(Debug, Clone, Serialize, Deserialize, zlink::introspect::Type)]
    pub struct OpenReply {
        #[serde(rename = "ptyFileDescriptor")]
        pub pty_file_descriptor: i64,
        #[serde(rename = "ptyPath")]
        pub pty_path: String,
    }

    /// Mock systemd-machined service that serves hardcoded responses.
    #[derive(Default)]
    pub struct MockMachinedService;

    impl MockMachinedService {
        /// Create a new mock machined service.
        pub fn new() -> Self {
            Self
        }
    }

    #[zlink::service(
        interface = "io.systemd.Machine",
        vendor = "The systemd Project",
        product = "systemd (systemd-machined)",
        version = "257.5 (257.5-6.fc42)",
        url = "https://systemd.io/",
        types = [AcquireMetadata, MachineOpenMode, ProcessId, Timestamp, Address]
    )]
    impl<Sock> MockMachinedService
    where
        Sock::ReadHalf: FetchPeerCredentials,
    {
        #[allow(clippy::too_many_arguments)]
        async fn register(
            &self,
            _name: String,
            _id: Option<String>,
            _service: Option<String>,
            _class: String,
            _leader: Option<u32>,
            #[zlink(rename = "leaderProcessId")] _leader_process_id: Option<ProcessId>,
            #[zlink(rename = "rootDirectory")] _root_directory: Option<String>,
            #[zlink(rename = "ifIndices")] _if_indices: Option<Vec<u64>>,
            #[zlink(rename = "vSockCid")] _v_sock_cid: Option<u64>,
            #[zlink(rename = "sshAddress")] _ssh_address: Option<String>,
            #[zlink(rename = "sshPrivateKeyPath")] _ssh_private_key_path: Option<String>,
            #[zlink(rename = "allocateUnit")] _allocate_unit: Option<bool>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<(), MachinedError> {
            verify_credentials(conn).await?;
            Ok(())
        }

        async fn unregister(
            &self,
            _name: Option<String>,
            _pid: Option<ProcessId>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<(), MachinedError> {
            verify_credentials(conn).await?;
            Ok(())
        }

        async fn terminate(
            &self,
            _name: Option<String>,
            _pid: Option<ProcessId>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<(), MachinedError> {
            verify_credentials(conn).await?;
            Ok(())
        }

        async fn kill(
            &self,
            _name: Option<String>,
            _pid: Option<ProcessId>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            _whom: Option<String>,
            _signal: i64,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<(), MachinedError> {
            verify_credentials(conn).await?;
            Ok(())
        }

        async fn list(
            &self,
            _name: Option<String>,
            _pid: Option<ProcessId>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            #[zlink(rename = "acquireMetadata")] _acquire_metadata: Option<AcquireMetadata>,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<ListReply<'static>, MachinedError> {
            verify_credentials(conn).await?;
            Ok(MOCK_LIST_REPLY)
        }

        #[allow(clippy::too_many_arguments)]
        async fn open(
            &self,
            _name: Option<String>,
            _pid: Option<ProcessId>,
            #[zlink(rename = "allowInteractiveAuthentication")]
            _allow_interactive_authentication: Option<bool>,
            _mode: MachineOpenMode,
            _user: Option<String>,
            _path: Option<String>,
            _args: Option<Vec<String>>,
            _environment: Option<Vec<String>>,
            #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
        ) -> Result<OpenReply, MachinedError> {
            verify_credentials(conn).await?;
            Ok(OpenReply {
                pty_file_descriptor: 42,
                pty_path: "/dev/pts/42".to_string(),
            })
        }
    }

    async fn verify_credentials<Sock>(
        conn: &mut zlink::Connection<Sock>,
    ) -> Result<(), MachinedError>
    where
        Sock: zlink::connection::Socket,
        Sock::ReadHalf: FetchPeerCredentials,
    {
        let Ok(creds) = conn.peer_credentials().await else {
            return Err(MachinedError::FetchPeerCredentialsFailed);
        };
        // Verify credentials match current process.
        if creds_utils::verify_credentials(creds).is_err() {
            return Err(MachinedError::FetchPeerCredentialsFailed);
        }
        Ok(())
    }

    const MOCK_LIST_REPLY: ListReply<'static> = ListReply {
        name: ".host",
        id: Some("1234567890abcdef1234567890abcdef"),
        service: Some("mock-service"),
        class: "host",
        leader: Some(super::ProcessId {
            pid: 12345,
            pidfd_id: None,
            boot_id: None,
        }),
        root_directory: Some("/var/lib/machines/test-machine"),
        unit: Some(Cow::Borrowed("machine-test\\x2dmachine.scope")),
        timestamp: Some(Timestamp {
            realtime: Some(1234567890000000),
            monotonic: Some(9876543210000),
        }),
        v_sock_cid: None,
        ssh_address: None,
        ssh_private_key_path: None,
        addresses: None,
        os_release: None,
        uid_shift: None,
    };
}

#[cfg(feature = "server")]
pub use server::MockMachinedService;
