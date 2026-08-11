#![cfg(feature = "serve")]
// This module is pulled into several independent test binaries via
// `mod harness;`. Each binary only exercises a subset of these helpers, so a
// helper unused by one binary is not dead code in the crate as a whole.
#![allow(dead_code)]

use ephemeris::commit::{CommitQueue, Committer};
use ephemeris::serve::Server;
use ephemeris::store::Store;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// A server on a socket in a temp dir, shut down on drop.
pub struct TestServer {
    path: PathBuf,
    /// Owned only when `start()` created its own temp directory. `start_at`
    /// binds inside a directory the caller owns and keeps alive itself.
    _dir: Option<tempfile::TempDir>,
    shutdown: Option<ephemeris::serve::ShutdownHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    /// Start a server on a socket inside a fresh temp directory, which also
    /// serves as the store directory.
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eph.sock");
        let store_dir = dir.path().to_path_buf();
        Self::start_with(&store_dir, &path, Some(dir))
    }

    /// Bind at a specific socket path with an explicit store directory. The
    /// caller owns the directory's lifetime — used to bind over a
    /// deliberately pre-created (stale) socket file.
    pub fn start_at(store_dir: &Path, socket_path: &Path) -> Self {
        Self::start_with(store_dir, socket_path, None)
    }

    /// A server with its connection cap and per-request deadline overridden,
    /// instead of the production `MAX_CONNECTIONS`/`REQUEST_DEADLINE`. Exists
    /// so those limits can be exercised without opening hundreds of real
    /// connections or sleeping through a 30-second production deadline.
    pub fn start_with_limits(
        max_connections: usize,
        request_deadline: std::time::Duration,
    ) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eph.sock");
        let store = Store::open(dir.path()).expect("store");
        let queue = CommitQueue::new(Committer::new(store));
        let mut server = Server::bind(&path, queue, dir.path()).expect("bind");
        server.set_limits_for_test(max_connections, request_deadline);
        let shutdown = server.shutdown_handle();
        let worker = std::thread::spawn(move || server.serve_until_shutdown());
        Self {
            path,
            _dir: Some(dir),
            shutdown: Some(shutdown),
            worker: Some(worker),
        }
    }

    fn start_with(store_dir: &Path, socket_path: &Path, dir: Option<tempfile::TempDir>) -> Self {
        let store = Store::open(store_dir).expect("store");
        let queue = CommitQueue::new(Committer::new(store));
        let server = Server::bind(socket_path, queue, store_dir).expect("bind");
        let shutdown = server.shutdown_handle();
        let worker = std::thread::spawn(move || server.serve_until_shutdown());
        Self {
            path: socket_path.to_path_buf(),
            _dir: dir,
            shutdown: Some(shutdown),
            worker: Some(worker),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn store_dir(&self) -> &Path {
        match &self._dir {
            Some(dir) => dir.path(),
            None => panic!("store_dir is only meaningful for a TestServer that owns its dir"),
        }
    }

    /// A server whose committer was latched unavailable before it started.
    pub fn start_with_broken_store() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eph.sock");
        let mut committer = Committer::new(Store::open(dir.path()).expect("store"));
        let seg = dir.path().join("segment-0.log");
        std::fs::remove_file(&seg).expect("remove segment");
        std::fs::create_dir(&seg).expect("dir in its place");
        committer.force_reopen_for_test(dir.path());
        let server = Server::bind(&path, CommitQueue::new(committer), dir.path()).expect("bind");
        let shutdown = server.shutdown_handle();
        let worker = std::thread::spawn(move || server.serve_until_shutdown());
        Self {
            path,
            _dir: Some(dir),
            shutdown: Some(shutdown),
            worker: Some(worker),
        }
    }
}

/// POST a body and return (status, body).
pub fn post(sock: &Path, target: &str, body: &str) -> (u16, String) {
    request(
        sock,
        &format!(
            "POST {target} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// GET and return (status, body).
pub fn get(sock: &Path, target: &str) -> (u16, String) {
    request(sock, &format!("GET {target} HTTP/1.1\r\n\r\n"))
}

fn request(sock: &Path, raw: &str) -> (u16, String) {
    let mut s = UnixStream::connect(sock).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("timeout");
    s.write_all(raw.as_bytes()).expect("write");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status = out
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(s) = self.shutdown.take() {
            s.stop();
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
