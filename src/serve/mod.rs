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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("could not bind the socket at {path}: {reason}")]
    Bind { path: String, reason: String },
    #[error("could not set the socket's permissions at {path}: {reason}")]
    Permissions { path: String, reason: String },
}

/// Hard cap on live connections, counted from accept to the connection
/// thread's exit — whether that thread is mid-request or idling inside its
/// deadline. A real deployment is one `transit guard` process talking to
/// one socket; even generously assuming many requests in flight from it at
/// once, legitimate concurrency here is small. This is far above that and
/// still a hard stop well short of where thread creation itself starts
/// failing: the burst that motivated this cap opened 1500 connections in
/// under a second and crashed the accept loop by exhausting the OS thread
/// limit.
const MAX_CONNECTIONS: usize = 256;

/// How long one request — request line, headers, and body together — may
/// take from the moment its connection is accepted. Enforced as a
/// wall-clock budget inside `http::read_request`, not by the per-read
/// socket timeout alone: that timeout resets on every successful read, so
/// it does not bound a client that trickles data slowly but steadily.
/// Generous for a same-machine Unix socket, where a legitimate request
/// completes in milliseconds.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// Decrements the live-connection count when a connection thread exits, for
/// any reason — including unwinding past this on a panic inside
/// `routes::handle`. A `usize` guard rather than the thread simply
/// decrementing at the end of its closure, so a slot cannot leak if that
/// closure ever panics instead of returning.
struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
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
    /// Live connections right now. Incremented in the accept loop before a
    /// worker thread is spawned, decremented by `ActiveGuard` on that
    /// thread's exit.
    active: Arc<AtomicUsize>,
    max_connections: usize,
    request_deadline: Duration,
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
            active: Arc::new(AtomicUsize::new(0)),
            max_connections: MAX_CONNECTIONS,
            request_deadline: REQUEST_DEADLINE,
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

    /// Test-only hook: override the connection cap and per-request deadline
    /// a production `bind` sets from `MAX_CONNECTIONS`/`REQUEST_DEADLINE`.
    /// Not part of the serving API — it exists so a test can exercise both
    /// limits without opening hundreds of real connections or sleeping
    /// through a 30-second production deadline.
    #[doc(hidden)]
    pub fn set_limits_for_test(&mut self, max_connections: usize, request_deadline: Duration) {
        self.max_connections = max_connections;
        self.request_deadline = request_deadline;
    }

    /// Accept connections until shutdown. One thread per connection, capped
    /// at `max_connections` live at a time.
    pub fn serve_until_shutdown(self) {
        for incoming in self.listener.incoming() {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let stream = match incoming {
                Ok(s) => s,
                Err(_) => continue,
            };

            // A read timeout, set the moment a connection is accepted,
            // bounds a client that connects and never sends; otherwise one
            // such client parks a slot forever. `http::read_request`
            // tightens this further, per read, into a TOTAL deadline — see
            // its doc comment for why the two are not the same guarantee.
            let _ = stream.set_read_timeout(Some(self.request_deadline));

            let live = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            if live > self.max_connections {
                self.active.fetch_sub(1, Ordering::SeqCst);
                // Bounds the accept loop's OWN blocking: a client that will
                // not read its refusal must not be able to stall every
                // connection behind it.
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                http::write_response(
                    &stream,
                    503,
                    "Service Unavailable",
                    r#"{"error":"too many connections"}"#,
                );
                continue;
            }

            // Cloned so the accept loop keeps its own handle to the
            // connection. If `Builder::spawn` below fails, that handle is
            // what lets a 503 still be written, rather than the connection
            // silently vanishing.
            let worker_stream = match stream.try_clone() {
                Ok(s) => s,
                Err(_) => {
                    self.active.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
            };
            let queue = Arc::clone(&self.queue);
            let store_dir = self.store_dir.clone();
            let active = Arc::clone(&self.active);
            let request_deadline = self.request_deadline;

            // `Builder::spawn`, not `thread::spawn`: the latter PANICS when
            // the OS refuses a new thread, and that panic would land on
            // THIS accept-loop thread, which has no `catch_unwind`. A burst
            // that exhausts the OS thread limit must shed the one
            // connection that triggered it, not unwind the loop and stop
            // accepting permanently until the process is restarted.
            let spawned = std::thread::Builder::new().spawn(move || {
                // Decremented on every exit from this thread, including a
                // panic inside `routes::handle` unwinding past it.
                let _guard = ActiveGuard(active);
                routes::handle(&worker_stream, &queue, &store_dir, request_deadline);
            });

            if spawned.is_err() {
                self.active.fetch_sub(1, Ordering::SeqCst);
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                http::write_response(
                    &stream,
                    503,
                    "Service Unavailable",
                    r#"{"error":"could not start a worker thread"}"#,
                );
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
