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
fn a_header_line_over_the_cap_is_refused_not_buffered() {
    // `BufRead::read_line` grows its `String` without limit chasing a `\n`
    // that may never arrive — `MAX_BODY` only ever bounded the body, not
    // this. This sends a header line just over `MAX_LINE`, not the
    // multi-megabyte version that motivated the fix, so the test stays
    // fast; the mechanism it exercises is the same either way.
    let srv = TestServer::start();
    let pad = "a".repeat(ephemeris::serve::http::MAX_LINE + 100);
    let req = format!("GET /root HTTP/1.1\r\nX-Pad: {pad}\r\n\r\n");
    let resp = raw(srv.path(), req.as_bytes()).expect("no response");
    assert!(resp.starts_with("HTTP/1.1 431"), "{}", first_line(&resp));
}

#[test]
fn many_small_headers_over_the_total_cap_are_refused_not_buffered() {
    // No single header line here is anywhere near `MAX_LINE` — this
    // exercises the separate total-bytes cap, which bounds the NUMBER of
    // headers rather than any one header's length. `MAX_LINE` alone would
    // let this case straight through.
    let srv = TestServer::start();
    let header = "X-Pad: 1234567890\r\n";
    let count = ephemeris::serve::http::MAX_HEADERS / header.len() + 100;
    let mut req = String::from("GET /root HTTP/1.1\r\n");
    req.push_str(&header.repeat(count));
    req.push_str("\r\n");
    let resp = raw(srv.path(), req.as_bytes()).expect("no response");
    assert!(resp.starts_with("HTTP/1.1 431"), "{}", first_line(&resp));
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

#[test]
fn a_connection_over_the_cap_is_refused_with_503_not_queued_forever() {
    // A small injected cap and a short injected deadline, not the
    // production 256/30s — this must stay fast, not open hundreds of real
    // connections.
    let srv = TestServer::start_with_limits(2, std::time::Duration::from_millis(300));

    // Occupy both slots: connect but send nothing, so each connection's
    // thread is alive and simply blocked reading a request line.
    let _a = UnixStream::connect(srv.path()).expect("connect a");
    let _b = UnixStream::connect(srv.path()).expect("connect b");
    // Give the accept loop a moment to actually accept and count both
    // before the third connection below arrives.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let resp = raw(srv.path(), b"GET /root HTTP/1.1\r\n\r\n").expect("no response");
    assert!(resp.starts_with("HTTP/1.1 503"), "{}", first_line(&resp));
}

#[test]
fn a_trickling_client_is_cut_off_by_the_total_deadline_not_the_idle_timeout() {
    // Each individual gap (50ms) is well inside any reasonable idle timeout,
    // so only a TOTAL wall-clock deadline — not a per-read timer that resets
    // on every successful read — can catch this. The client keeps sending
    // for up to 1s, far longer than the 150ms deadline, so an
    // idle-timeout-only implementation (which would never notice as long as
    // gaps stay short) and a true total-deadline implementation (which cuts
    // off around 150ms regardless) are cleanly distinguishable by how long
    // the response actually takes to arrive — checked below, not assumed.
    let deadline = std::time::Duration::from_millis(150);
    let srv = TestServer::start_with_limits(usize::MAX, deadline);
    let mut writer = UnixStream::connect(srv.path()).expect("connect");
    writer
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("write timeout");
    let mut reader = writer.try_clone().expect("clone");
    reader
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("read timeout");

    let started = std::time::Instant::now();
    // Sends on its own thread so it keeps trickling independently of
    // whatever the reader below observes — the read must not be able to
    // finish early just because the writer is still sleeping between bytes.
    let sender = std::thread::spawn(move || {
        for _ in 0..20 {
            if writer.write_all(b"G").is_err() {
                break; // the server already closed the connection — expected
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    let mut out = String::new();
    let _ = reader.read_to_string(&mut out);
    let elapsed = started.elapsed();
    let _ = sender.join();

    assert!(out.starts_with("HTTP/1.1 408"), "{out:?}");
    // Generous above the 150ms deadline for scheduling slack, but far under
    // the ~1s the sender is prepared to keep trickling for — an
    // idle-timeout-only implementation would run past this bound because no
    // single 50ms gap ever trips it.
    assert!(
        elapsed < std::time::Duration::from_millis(600),
        "took {elapsed:?} to be cut off, expected close to the {deadline:?} deadline"
    );
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}
