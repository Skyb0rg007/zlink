//! Shared utilities for peer credentials verification in tests.

#![cfg(feature = "server")]

use rustix::process::Pid;
use zlink::connection::Credentials;

/// Verify that the given credentials match the current process.
///
/// Returns `Ok(())` if all checks pass, or `Err(message)` describing the failure.
pub fn verify_credentials(creds: &Credentials) -> Result<(), &'static str> {
    verify_uid(creds)?;
    verify_pid(creds)?;
    verify_gid(creds)?;
    #[cfg(target_os = "linux")]
    verify_pidfd(creds)?;
    Ok(())
}

/// Verify UID matches current process.
pub fn verify_uid(creds: &Credentials) -> Result<(), &'static str> {
    let expected_uid = rustix::process::getuid();
    if creds.unix_user_id() != expected_uid {
        return Err("UID does not match current process");
    }
    Ok(())
}

/// Verify GID matches current process.
pub fn verify_gid(creds: &Credentials) -> Result<(), &'static str> {
    let expected_gid = rustix::process::getgid();
    if creds.unix_primary_group_id() != expected_gid {
        return Err("GID does not match current process");
    }
    Ok(())
}

/// Verify PID matches current process.
///
/// On FreeBSD < 13 and DragonFly BSD, PID may be 0.
pub fn verify_pid(creds: &Credentials) -> Result<(), &'static str> {
    let expected_pid = rustix::process::getpid();
    let pid_ok = is_pid_valid(creds.process_id(), expected_pid);
    if !pid_ok {
        return Err("PID does not match current process");
    }
    Ok(())
}

/// Check if the credential PID matches expected, accounting for platform quirks.
fn is_pid_valid(actual: Pid, expected: Pid) -> bool {
    #[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
    {
        // FreeBSD 13+ has PID support, older versions return 0.
        // DragonFly BSD currently returns 0 (PID support TBD).
        actual == expected || actual == Pid::from_raw(0).unwrap()
    }
    #[cfg(not(any(target_os = "freebsd", target_os = "dragonfly")))]
    {
        actual == expected
    }
}

/// Verify pidfd refers to the current process (Linux only).
#[cfg(target_os = "linux")]
pub fn verify_pidfd(creds: &Credentials) -> Result<(), &'static str> {
    use std::os::fd::{AsFd, AsRawFd};

    let fd_num = creds.process_fd().as_fd().as_raw_fd();
    if fd_num < 0 {
        return Err("Process FD is invalid");
    }

    // Read /proc/self/fdinfo to verify pidfd refers to correct process.
    let fdinfo_path = format!("/proc/self/fdinfo/{}", fd_num);
    let fdinfo =
        std::fs::read_to_string(&fdinfo_path).map_err(|_| "Failed to read fdinfo for pidfd")?;

    let expected_pid = rustix::process::getpid();
    let pid_matches = fdinfo
        .lines()
        .find(|line| line.starts_with("Pid:"))
        .and_then(|line| line.strip_prefix("Pid:"))
        .and_then(|s| s.trim().parse::<i32>().ok())
        .and_then(Pid::from_raw)
        .is_some_and(|pid| pid == expected_pid);

    if !pid_matches {
        return Err("pidfd does not refer to the current process");
    }
    Ok(())
}
