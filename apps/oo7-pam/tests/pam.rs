#![cfg(target_os = "linux")]

mod common;
#[allow(dead_code)]
#[path = "../src/protocol.rs"]
mod protocol;

use std::{
    io::Read,
    os::unix::net::UnixListener,
    time::{Duration, Instant},
};

use common::PAM_SUCCESS;
use protocol::PamMessage;

/// The request must be fully written by the time PAM returns, the caller may
/// exit right after.
fn take_request(listener: &UnixListener) -> Vec<u8> {
    let (mut stream, _) = listener
        .accept()
        .expect("PAM returned before connecting to the daemon");
    stream.set_nonblocking(true).unwrap();
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .expect("PAM returned before sending the whole request");
    let (length, message) = bytes.split_at(4);
    assert_eq!(
        u32::from_le_bytes(length.try_into().unwrap()) as usize,
        message.len()
    );
    message.to_vec()
}

/// The daemon never answers, PAM must not wait for it.
fn assert_quick(start: Instant) {
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn open_session_delivers_unlock() {
    let fixture = common::fixture();
    let mut pam = fixture.start();
    let listener = fixture.listen();

    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    let start = Instant::now();
    assert_eq!(pam.open_session(), PAM_SUCCESS);
    assert_quick(start);

    let expected = PamMessage::unlock(fixture.username.clone(), b"old-password".to_vec());
    assert_eq!(take_request(&listener), expected.to_bytes().unwrap());
    assert_eq!(pam.close_session(), PAM_SUCCESS);
}

#[test]
fn chauthtok_delivers_password_change() {
    let fixture = common::fixture();
    let mut pam = fixture.start();
    let listener = fixture.listen();

    let start = Instant::now();
    assert_eq!(pam.chauthtok(), PAM_SUCCESS);
    assert_quick(start);

    let expected = PamMessage::change_password(
        fixture.username.clone(),
        b"old-password".to_vec(),
        b"new-password".to_vec(),
    );
    assert_eq!(take_request(&listener), expected.to_bytes().unwrap());
}

#[test]
fn missing_daemon_is_not_fatal() {
    let fixture = common::fixture();
    let mut pam = fixture.start();

    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    assert_eq!(pam.open_session(), PAM_SUCCESS);
    assert_eq!(pam.close_session(), PAM_SUCCESS);
    assert_eq!(pam.chauthtok(), PAM_SUCCESS);
}

const AUTH_ONLY: &str = "auth required @tokens@\nauth optional @module@ unlock_on_auth\n";

fn assert_no_request(listener: &UnixListener) {
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn auth_only_delivers_at_successful_pam_end_without_opening_session() {
    let fixture = common::fixture();
    let mut pam = fixture.start_auth(AUTH_ONLY);
    let listener = fixture.listen();
    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    assert_no_request(&listener);
    let start = Instant::now();
    drop(pam);
    assert_quick(start);
    let expected = PamMessage::unlock(fixture.username.clone(), b"old-password".to_vec());
    assert_eq!(take_request(&listener), expected.to_bytes().unwrap());
}

#[test]
fn failed_authentication_never_delivers_even_with_a_valid_keyring_password() {
    let fixture = common::fixture();
    for policy in [
        "auth required @tokens@ reject\nauth optional @module@ unlock_on_auth\n",
        "auth required @tokens@\nauth optional @module@ unlock_on_auth\nauth required @tokens@ reject\n",
    ] {
        let mut pam = fixture.start_auth(policy);
        let listener = fixture.listen();
        assert_ne!(pam.authenticate(), PAM_SUCCESS);
        drop(pam);
        assert_no_request(&listener);
    }
}

#[test]
fn auth_only_is_opt_in() {
    let fixture = common::fixture();
    let mut pam = fixture.start_auth("auth required @tokens@\nauth optional @module@\n");
    let listener = fixture.listen();
    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    drop(pam);
    assert_no_request(&listener);
}

#[test]
fn missing_and_empty_passwords_do_not_send_or_retain_an_earlier_token() {
    let fixture = common::fixture();
    for option in ["no_token", "empty_token"] {
        for previous in ["", AUTH_ONLY] {
            let policy = format!(
                "{previous}auth required @tokens@ {option}\nauth optional @module@ unlock_on_auth\n"
            );
            let mut pam = fixture.start_auth(&policy);
            let listener = fixture.listen();
            assert_eq!(pam.authenticate(), PAM_SUCCESS);
            drop(pam);
            assert_no_request(&listener);
        }
    }
}

#[test]
fn repeated_module_replaces_pending_data_without_early_delivery() {
    let fixture = common::fixture();
    let mut pam = fixture.start_auth(&format!("{AUTH_ONLY}{AUTH_ONLY}"));
    let listener = fixture.listen();
    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    assert_no_request(&listener);
    drop(pam);
    take_request(&listener);
    assert_no_request(&listener);
}

#[test]
fn unavailable_or_invalid_socket_does_not_change_authentication() {
    let fixture = common::fixture();
    for fake_socket in [false, true] {
        let mut pam = fixture.start_auth(AUTH_ONLY);
        if fake_socket {
            std::fs::write(&fixture.socket_path, "not a socket").unwrap();
        }
        assert_eq!(pam.authenticate(), PAM_SUCCESS);
        let start = Instant::now();
        drop(pam);
        assert_quick(start);
        if fake_socket {
            assert_eq!(
                std::fs::read_to_string(&fixture.socket_path).unwrap(),
                "not a socket"
            );
        }
    }
}

#[test]
fn host_substack_preserves_sufficient_shortcuts_and_required_failures() {
    let fixture = common::fixture();
    let policy = "auth substack host-auth\nauth optional @module@ unlock_on_auth\n";
    fixture.write_policy(
        "host-auth",
        "auth sufficient @tokens@\nauth requisite @tokens@ reject\n",
    );
    let mut pam = fixture.start_auth(policy);
    let listener = fixture.listen();
    assert_eq!(pam.authenticate(), PAM_SUCCESS);
    assert_no_request(&listener);
    drop(pam);
    take_request(&listener);
    drop(listener);

    fixture.write_policy("host-auth", "auth required @tokens@ reject\n");
    let mut pam = fixture.start_auth(policy);
    let listener = fixture.listen();
    assert_ne!(pam.authenticate(), PAM_SUCCESS);
    drop(pam);
    assert_no_request(&listener);
}
