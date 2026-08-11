#![cfg(feature = "serve")]

use ephemeris::commit::{CommitQueue, Committer};
use ephemeris::serve::Server;
use ephemeris::store::Store;
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
