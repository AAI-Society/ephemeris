use crate::commit::CommitQueue;
use crate::serve::http::{read_request, write_response, HttpError};
use std::os::unix::net::UnixStream;

/// Dispatch one request. Never panics: every parse failure becomes a status.
pub fn handle(stream: &UnixStream, _queue: &CommitQueue) {
    let req = match read_request(stream) {
        Ok(r) => r,
        Err(HttpError::BodyTooLarge { .. }) => {
            return write_response(
                stream,
                413,
                "Payload Too Large",
                r#"{"error":"body too large"}"#,
            )
        }
        Err(HttpError::Incomplete) => return,
        Err(e) => {
            return write_response(
                stream,
                400,
                "Bad Request",
                &format!(r#"{{"error":{}}}"#, json_string(&e.to_string())),
            )
        }
    };

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/root") => write_response(stream, 200, "OK", r#"{"todo":"task 3"}"#),
        ("GET", _) | ("POST", _) => {
            write_response(stream, 404, "Not Found", r#"{"error":"no such route"}"#)
        }
        _ => write_response(
            stream,
            405,
            "Method Not Allowed",
            r#"{"error":"method not allowed"}"#,
        ),
    }
}

/// Minimal JSON string escaping, so an error message cannot break the body it
/// is embedded in. `serde_json` does this properly and is used for every real
/// payload; this exists only for the error path, which must never itself fail.
pub(crate) fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
