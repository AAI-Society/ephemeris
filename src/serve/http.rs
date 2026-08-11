use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// A claim is a small JSON object. Anything an order of magnitude larger is
/// not a claim, and bounding before allocation is the same discipline
/// `store::MAX_RECORD` applies to bytes off the disk.
pub const MAX_BODY: usize = 64 * 1024;

/// A single request line or header line longer than this is refused before
/// more of it is buffered looking for a terminator. `MAX_BODY` only ever
/// bounded the body: reading a line without a cap otherwise grows without
/// limit chasing a `\n` that may never arrive, so a client that sends one
/// unterminated header line and nothing else is bounded by nothing at all.
/// Generous for this API: the longest legitimate request line looks like
/// `GET /proof/consistency?from=18446744073709551615&to=18446744073709551615 HTTP/1.1`
/// — a couple hundred bytes — and the only header this server reads is
/// `Content-Length: <up to 20 digits>`. A few KB leaves headroom for any real
/// client while making an unbounded line impossible.
pub const MAX_LINE: usize = 8 * 1024;

/// Total bytes across every header line, not counting the request line or
/// the terminating blank line. `MAX_LINE` alone still allows an unbounded
/// NUMBER of headers as long as each one is short by itself; this bounds the
/// count too. Small next to `MAX_BODY`, generous for the one header
/// (`Content-Length`) a real client here ever needs to send.
pub const MAX_HEADERS: usize = 32 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("the connection closed before a complete request arrived")]
    Incomplete,
    #[error("the request line was not `METHOD PATH VERSION`")]
    BadRequestLine,
    #[error("the request line is longer than {MAX_LINE} bytes")]
    RequestLineTooLong,
    #[error(
        "the request headers are longer than {MAX_HEADERS} bytes in total, \
         or one header line exceeded {MAX_LINE} bytes"
    )]
    HeadersTooLarge,
    #[error("Content-Length was not a number within bounds")]
    BadContentLength,
    #[error("the body is {len} bytes, over the {MAX_BODY}-byte limit")]
    BodyTooLarge { len: usize },
    #[error("the request did not complete within its deadline")]
    DeadlineExceeded,
    #[error("could not read the request: {0}")]
    Io(String),
}

pub struct Request {
    pub method: String,
    /// Path with any query string removed.
    pub path: String,
    /// Raw query string, empty when absent.
    pub query: String,
    pub body: Vec<u8>,
}

impl Request {
    /// The value of a query parameter, or `None`. Deliberately tiny: the two
    /// proof routes take integers and nothing else.
    pub fn param(&self, key: &str) -> Option<&str> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if k == key {
                Some(v)
            } else {
                None
            }
        })
    }

    /// A query parameter parsed as `u64`. `None` if absent or not a number —
    /// never a panic, and never a silent zero.
    pub fn u64_param(&self, key: &str) -> Option<u64> {
        self.param(key)?.parse().ok()
    }
}

/// The outcome of reading one line with a hard cap on how far to look for
/// its terminator.
enum Line {
    Complete(String),
    /// The cap was reached before a `\n` was found — the line itself, not
    /// just the connection, is the problem.
    TooLong,
    /// The connection ended before a full line arrived.
    Incomplete,
}

/// Classify a failed read. The only thing that ever sets a read timeout on
/// this socket is `tighten_to_deadline`, below, so a `WouldBlock`/`TimedOut`
/// error here can only mean the deadline was reached mid-read — the socket
/// timeout that fired IS the deadline, not some separate idle timer. Every
/// other error is a genuine I/O failure.
fn classify_read_err(e: std::io::Error) -> HttpError {
    if matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ) {
        HttpError::DeadlineExceeded
    } else {
        HttpError::Io(e.to_string())
    }
}

/// Shrink the stream's read timeout to whatever remains of `deadline`,
/// immediately before a read. This is what makes the deadline TOTAL rather
/// than per-read: `set_read_timeout` alone resets on every successful read,
/// so a client that keeps sending a few bytes just often enough would never
/// trip it. Returns an error, without touching the socket, once the
/// deadline has already passed.
fn tighten_to_deadline(stream: &UnixStream, deadline: Instant) -> Result<(), HttpError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(HttpError::DeadlineExceeded);
    }
    let _ = stream.set_read_timeout(Some(remaining));
    Ok(())
}

/// Read one line, one byte at a time, tightening the deadline before EVERY
/// byte — not just once before the whole line.
///
/// A buffered, multi-byte reader (`BufRead::read_line`) cannot give this
/// guarantee: it loops internally across as many individual reads as it
/// takes to find a `\n`, and a deadline tightened once before that whole
/// call only bounds each of ITS internal reads individually, not the call's
/// cumulative duration. That gap is exactly how a client sending one byte
/// every few tens of milliseconds can trickle straight past a nominal
/// deadline that then never fires, because no single underlying read within
/// it ever times out. Reading a byte at a time, with its own deadline check
/// in front of it, closes that gap: the check that matters is "how much
/// budget is left in total", asked before every syscall, not "did this one
/// read take too long."
///
/// Byte-at-a-time costs a syscall per byte instead of per buffer-full. This
/// API's lines are tiny (a request line and the one header this server reads
/// are both well under 200 bytes in the common case) and the crate's
/// measured ceiling is `fsync`, not the transport — see the design's
/// transport section — so that cost is not a real one here.
fn read_line_capped(
    mut stream: &UnixStream,
    deadline: Instant,
    cap: usize,
) -> Result<Line, HttpError> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        tighten_to_deadline(stream, deadline)?;
        let n = stream.read(&mut byte).map_err(classify_read_err)?;
        if n == 0 {
            return Ok(Line::Incomplete);
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return match String::from_utf8(line) {
                Ok(s) => Ok(Line::Complete(s)),
                Err(_) => Err(HttpError::Io(
                    "the request line was not valid UTF-8".to_string(),
                )),
            };
        }
        if line.len() >= cap {
            return Ok(Line::TooLong);
        }
    }
}

/// Read exactly `buf.len()` bytes, tightening the deadline before every
/// underlying read — the body's equivalent of `read_line_capped`'s
/// byte-at-a-time loop, for the same reason: `Read::read_exact` loops
/// internally, and a deadline set once before that whole call would only
/// bound each internal read, not the call's cumulative duration.
fn read_body_deadlined(
    mut stream: &UnixStream,
    deadline: Instant,
    len: usize,
) -> Result<Vec<u8>, HttpError> {
    let mut body = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        tighten_to_deadline(stream, deadline)?;
        match stream.read(&mut body[filled..]) {
            Ok(0) => return Err(HttpError::Incomplete),
            Ok(n) => filled += n,
            Err(e) => return Err(classify_read_err(e)),
        }
    }
    Ok(body)
}

/// Read one HTTP/1.1 request. Every failure is an error, never a panic.
///
/// `deadline` bounds the WHOLE request — request line, headers, and body
/// together — as wall-clock time, not as an idle timer that resets on every
/// byte received.
pub fn read_request(stream: &UnixStream, deadline: Duration) -> Result<Request, HttpError> {
    let deadline = Instant::now() + deadline;

    let line = match read_line_capped(stream, deadline, MAX_LINE)? {
        Line::Complete(l) => l,
        Line::TooLong => return Err(HttpError::RequestLineTooLong),
        Line::Incomplete => return Err(HttpError::Incomplete),
    };
    let mut parts = line.trim_end().split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("");
    if method.is_empty() || target.is_empty() || version.is_empty() {
        return Err(HttpError::BadRequestLine);
    }

    let mut content_length: usize = 0;
    let mut headers_total: usize = 0;
    loop {
        let h = match read_line_capped(stream, deadline, MAX_LINE)? {
            Line::Complete(l) => l,
            Line::TooLong => return Err(HttpError::HeadersTooLarge),
            Line::Incomplete => return Err(HttpError::Incomplete),
        };
        headers_total += h.len();
        if headers_total > MAX_HEADERS {
            return Err(HttpError::HeadersTooLarge);
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| HttpError::BadContentLength)?;
                if content_length > MAX_BODY {
                    return Err(HttpError::BodyTooLarge {
                        len: content_length,
                    });
                }
            }
        }
    }

    let body = if content_length > 0 {
        read_body_deadlined(stream, deadline, content_length)?
    } else {
        Vec::new()
    };

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };

    Ok(Request {
        method,
        path,
        query,
        body,
    })
}

pub fn write_response(mut stream: &UnixStream, status: u16, reason: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // A client that hung up mid-response is not an error worth escalating:
    // the record's durability was decided before the write was attempted.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}
