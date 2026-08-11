use crate::commit::CommitQueue;
use crate::serve::http::{read_request, write_response, HttpError, Request};
use std::os::unix::net::UnixStream;

/// Dispatch one request. Never panics: every parse failure becomes a status.
pub fn handle(stream: &UnixStream, queue: &CommitQueue) {
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
        ("POST", "/claims") => post_claims(stream, queue, &req),
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

/// `POST /claims?now=<unix seconds>` — submit one claim and block until it is
/// durable.
///
/// **The listener relays an `Ack`; it never builds one.** `QueueHandle::submit`
/// returns `Result<Ack, CommitError>` and only the `Ok` arm produces a 200. A
/// guard receiving anything else must not forward, and this route gives it no
/// way to confuse the two.
fn post_claims(stream: &UnixStream, queue: &CommitQueue, req: &Request) {
    let Some(now_secs) = req.u64_param("now") else {
        return write_response(
            stream,
            400,
            "Bad Request",
            r#"{"error":"missing or invalid `now` query parameter; time is injected, never read here"}"#,
        );
    };

    let claim: crate::record::Claim = match serde_json::from_slice(&req.body) {
        Ok(c) => c,
        Err(e) => {
            return write_response(
                stream,
                400,
                "Bad Request",
                &format!(r#"{{"error":{}}}"#, json_string(&e.to_string())),
            )
        }
    };

    match queue.handle().submit(claim, now_secs) {
        Ok(ack) => {
            let body = format!(
                r#"{{"step_index":{},"chain_head":{}}}"#,
                ack.step_index(),
                json_string(ack.chain_head())
            );
            write_response(stream, 200, "OK", &body)
        }
        Err(e) => write_response(
            stream,
            503,
            "Service Unavailable",
            &format!(r#"{{"error":{}}}"#, json_string(&e.to_string())),
        ),
    }
}

/// Minimal JSON string escaping, so an error message cannot break the body it
/// is embedded in. `serde_json` does this properly and is used for every real
/// payload; this exists only for the error path, which must never itself fail.
pub(crate) fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
