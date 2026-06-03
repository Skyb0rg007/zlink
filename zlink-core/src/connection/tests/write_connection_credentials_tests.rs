//! Unit tests for WriteConnection credential passing.

#![cfg(all(test, target_os = "linux"))]

use crate::{
    Call,
    connection::{PassedCredentials, write_connection::WriteConnection},
    test_utils::mock_socket::MockWriteHalf,
};
use rustix::process::{Gid, Pid, Uid};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum TestMethod {
    #[serde(rename = "org.example.Test")]
    Test { value: u32 },
}

fn sample_creds() -> PassedCredentials {
    // SAFETY: 1 is a valid PID.
    let pid = Pid::from_raw(1).unwrap();
    PassedCredentials::new(Uid::ROOT, Gid::ROOT, pid)
}

#[tokio::test]
async fn no_credentials_by_default() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);
    let call = Call::new(TestMethod::Test { value: 1 });

    write_conn.send_call(&call, vec![]).await.unwrap();

    assert!(write_conn.write_half().credentials_written().is_empty());
}

#[tokio::test]
async fn credentials_sent_after_set() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);
    write_conn.set_credentials(Some(sample_creds()));

    let call = Call::new(TestMethod::Test { value: 1 });
    write_conn.send_call(&call, vec![]).await.unwrap();

    let written = write_conn.write_half().credentials_written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].unix_user_id(), Uid::ROOT);
}

#[tokio::test]
async fn credentials_cleared_on_set_none() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);
    write_conn.set_credentials(Some(sample_creds()));

    let call = Call::new(TestMethod::Test { value: 1 });
    write_conn.send_call(&call, vec![]).await.unwrap();

    write_conn.set_credentials(None);
    write_conn.send_call(&call, vec![]).await.unwrap();

    let written = write_conn.write_half().credentials_written();
    // Only the first call should have recorded credentials.
    assert_eq!(written.len(), 1);
}
