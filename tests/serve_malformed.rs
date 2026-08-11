#![cfg(feature = "serve")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

mod harness;
use harness::TestServer;

/// Send raw bytes and read whatever comes back. Returns None if the server
/// closed without replying, which is an acceptable answer to garbage — what is
/// NOT acceptable is a panic, and a panicked server thread would fail the
/// later requests in each test.
fn raw(path: &std::path::Path, bytes: &[u8]) -> Option<String> {
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    s.write_all(bytes).ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[test]
fn garbage_does_not_panic_the_server() {
    let srv = TestServer::start();
    for bytes in [
        &b""[..],
        &b"\r\n\r\n"[..],
        &b"NOTAMETHOD / HTTP/1.1\r\n\r\n"[..],
        &b"GET"[..],
        &b"GET /root"[..],
        &b"GET /root HTTP/1.1\r\nContent-Length: notanumber\r\n\r\n"[..],
        &b"POST /claims HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n"[..],
        &b"POST /claims HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort"[..],
        &[0xff, 0xfe, 0xfd, 0x00, 0x01][..],
    ] {
        let _ = raw(srv.path(), bytes);
    }
    // The server must still be alive and answering. If any of the above
    // panicked its connection thread, this still passes — but if one poisoned
    // the listener, it does not.
    let after = raw(srv.path(), b"GET /root HTTP/1.1\r\n\r\n").expect("server died");
    assert!(
        after.starts_with("HTTP/1.1 "),
        "not an HTTP response: {after:?}"
    );
}

#[test]
fn an_oversized_body_is_refused_before_it_is_allocated() {
    let srv = TestServer::start();
    let body = "x".repeat(4096);
    let req = format!(
        "POST /claims HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
        2 * 1024 * 1024,
        body
    );
    let resp = raw(srv.path(), req.as_bytes()).expect("no response");
    assert!(
        resp.starts_with("HTTP/1.1 413"),
        "expected 413, got: {}",
        first_line(&resp)
    );
}

#[test]
fn an_unknown_path_is_404_and_an_unknown_method_is_405() {
    let srv = TestServer::start();
    let a = raw(srv.path(), b"GET /nope HTTP/1.1\r\n\r\n").expect("no response");
    assert!(a.starts_with("HTTP/1.1 404"), "{}", first_line(&a));
    let b = raw(srv.path(), b"DELETE /root HTTP/1.1\r\n\r\n").expect("no response");
    assert!(b.starts_with("HTTP/1.1 405"), "{}", first_line(&b));
}

#[test]
fn the_socket_is_not_world_writable() {
    // The design says filesystem permissions ARE the access control. If the
    // socket is group- or world-writable, there is no access control.
    use std::os::unix::fs::PermissionsExt;
    let srv = TestServer::start();
    let mode = std::fs::metadata(srv.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket mode is {mode:o}, not 0600");
}

#[test]
fn a_stale_socket_file_does_not_prevent_binding() {
    // A crashed process leaves the socket file behind. Refusing to start with
    // EADDRINUSE would turn a crash into an outage requiring manual cleanup.
    //
    // A previous `TestServer` dropping is not a faithful way to produce this
    // condition here: its temp directory is removed on drop, taking the
    // socket path with it, so nothing stale would actually remain at bind
    // time. Instead, create the leftover file directly and confirm it is
    // still there right before binding over it.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("eph.sock");
    std::fs::write(&path, b"stale").expect("write stale file");
    assert!(
        path.exists(),
        "the stale file must be present before binding"
    );

    let srv = TestServer::start_at(dir.path(), &path);
    assert!(raw(srv.path(), b"GET /root HTTP/1.1\r\n\r\n").is_some());
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}
