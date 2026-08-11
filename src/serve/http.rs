use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

/// A claim is a small JSON object. Anything an order of magnitude larger is
/// not a claim, and bounding before allocation is the same discipline
/// `store::MAX_RECORD` applies to bytes off the disk.
pub const MAX_BODY: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("the connection closed before a complete request arrived")]
    Incomplete,
    #[error("the request line was not `METHOD PATH VERSION`")]
    BadRequestLine,
    #[error("Content-Length was not a number within bounds")]
    BadContentLength,
    #[error("the body is {len} bytes, over the {MAX_BODY}-byte limit")]
    BodyTooLarge { len: usize },
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

/// Read one HTTP/1.1 request. Every failure is an error, never a panic.
pub fn read_request(stream: &UnixStream) -> Result<Request, HttpError> {
    let mut reader = BufReader::new(stream);

    let mut line = String::new();
    if reader.read_line(&mut line).map_err(io)? == 0 {
        return Err(HttpError::Incomplete);
    }
    let mut parts = line.trim_end().split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("");
    if method.is_empty() || target.is_empty() || version.is_empty() {
        return Err(HttpError::BadRequestLine);
    }

    let mut content_length: usize = 0;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).map_err(io)? == 0 {
            return Err(HttpError::Incomplete);
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

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|_| HttpError::Incomplete)?;
    }

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

fn io(e: std::io::Error) -> HttpError {
    HttpError::Io(e.to_string())
}
