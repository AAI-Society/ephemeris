//! The Unix-domain-socket listener. Behind the `serve` feature.
//!
//! One thread per connection, no async runtime. See the design's transport
//! section: this crate blocks on `fsync` for milliseconds per batch, and the
//! measured ceiling is durability rather than transport, so an executor would
//! add a `spawn_blocking` correctness hazard and buy nothing.

pub mod http;
pub mod routes;

use crate::commit::CommitQueue;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("could not bind the socket at {path}: {reason}")]
    Bind { path: String, reason: String },
    #[error("could not set the socket's permissions at {path}: {reason}")]
    Permissions { path: String, reason: String },
}

/// Stops a running server. Cloneable so a test can hold one while the server
/// owns itself on another thread.
#[derive(Clone)]
pub struct ShutdownHandle {
    flag: Arc<AtomicBool>,
    path: PathBuf,
}

impl ShutdownHandle {
    pub fn stop(&self) {
        self.flag.store(true, Ordering::SeqCst);
        // Unblock the accept loop by connecting to it once.
        let _ = std::os::unix::net::UnixStream::connect(&self.path);
    }
}

pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    queue: Arc<CommitQueue>,
    /// Where the durable store lives on disk. Not read by Task 1: the read
    /// routes added in Task 3 re-open the store from here per request, so a
    /// served proof can only ever cover records that are durable — see the
    /// module-level rationale in the task brief. Carried from `bind` onward
    /// so that adding those routes later does not change this signature.
    store_dir: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Bind the socket and take ownership of the commit queue.
    ///
    /// A stale socket file left by a crashed process is removed first. Refusing
    /// to start with `EADDRINUSE` would turn a crash into an outage needing
    /// manual cleanup, and the file is not evidence of a live process.
    ///
    /// `store_dir` is kept separate from the queue on purpose: see the
    /// `store_dir` field doc.
    pub fn bind(path: &Path, queue: CommitQueue, store_dir: &Path) -> Result<Server, ServeError> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path).map_err(|e| ServeError::Bind {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

        // Filesystem permissions ARE the access control here — anything that
        // can write to this socket can insert records. 0600, always.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            ServeError::Permissions {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Server {
            listener,
            path: path.to_path_buf(),
            queue: Arc::new(queue),
            store_dir: store_dir.to_path_buf(),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn local_path(&self) -> &Path {
        &self.path
    }

    /// The store directory this server was bound with. Unused until Task 3's
    /// read routes re-open the store from it per request.
    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle {
            flag: Arc::clone(&self.stop),
            path: self.path.clone(),
        }
    }

    /// Accept connections until shutdown. One thread per connection.
    pub fn serve_until_shutdown(self) {
        for incoming in self.listener.incoming() {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let stream = match incoming {
                Ok(s) => s,
                Err(_) => continue,
            };
            let queue = Arc::clone(&self.queue);
            let store_dir = self.store_dir.clone();
            // A read timeout bounds a client that connects and never sends;
            // otherwise one such client parks a thread forever.
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
            std::thread::spawn(move || routes::handle(&stream, &queue, &store_dir));
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
