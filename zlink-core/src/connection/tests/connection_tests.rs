//! Tests for Connection-level functionality.

use crate::{Connection, test_utils::mock_socket::MockSocket};

#[tokio::test]
#[cfg(feature = "std")]
async fn peer_credentials_mock_socket() {
    // Get the expected credentials of the current process.
    let expected_uid = rustix::process::getuid();
    let expected_pid = rustix::process::getpid();

    // Create a mock socket with some dummy responses.
    let socket = MockSocket::with_responses(&[]);
    let mut connection = Connection::new(socket);

    // Get peer credentials - should return current process credentials.
    let creds = connection.peer_credentials().await.unwrap();

    // Verify we got the correct credentials.
    assert_eq!(
        creds.unix_user_id(),
        expected_uid,
        "UID should match current process"
    );

    // On Linux/Android, mock socket returns actual PID. On other platforms, it returns actual PID
    // too (unlike real sockets which return 0).
    assert_eq!(
        creds.process_id(),
        expected_pid,
        "PID should match current process"
    );

    // Save values for comparison.
    let uid1 = creds.unix_user_id();
    let pid1 = creds.process_id();

    // Verify caching works - calling again should return the same values.
    let creds2 = connection.peer_credentials().await.unwrap();
    assert_eq!(uid1, creds2.unix_user_id(), "Cached UID should match");
    assert_eq!(pid1, creds2.process_id(), "Cached PID should match");
}
