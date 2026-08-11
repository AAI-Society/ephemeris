use crate::commit::CommitQueue;
use crate::record::digest;
use crate::serve::http::{read_request, write_response, HttpError, Request};
use crate::sign::{Signer, SoftwareSigner};
use crate::store::Store;
use crate::tree::{consistency_proof, inclusion_proof, leaf_hash, root_of};
use crate::verify::against_root;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// The same fixed seed the CLI uses, for the same reason: Phase 1 output is
/// reproducible and the key is a labelled placeholder either way.
const DEV_SEED: [u8; 32] = [7u8; 32];

/// Dispatch one request. Never panics: every parse failure becomes a status.
///
/// `request_deadline` bounds the WHOLE request over wall-clock time — see
/// `http::read_request` — and is threaded through from `Server` rather than
/// hardcoded here, so a test can shrink it without waiting on the
/// production value.
pub fn handle(
    stream: &UnixStream,
    queue: &CommitQueue,
    store_dir: &Path,
    request_deadline: Duration,
) {
    let req = match read_request(stream, request_deadline) {
        Ok(r) => r,
        Err(HttpError::BodyTooLarge { .. }) => {
            return write_response(
                stream,
                413,
                "Payload Too Large",
                r#"{"error":"body too large"}"#,
            )
        }
        Err(HttpError::HeadersTooLarge) => {
            return write_response(
                stream,
                431,
                "Request Header Fields Too Large",
                r#"{"error":"request headers too large"}"#,
            )
        }
        Err(HttpError::DeadlineExceeded) => {
            return write_response(
                stream,
                408,
                "Request Timeout",
                r#"{"error":"the request did not complete within its deadline"}"#,
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
        ("GET", "/root") => get_root(stream, store_dir),
        ("GET", "/proof/inclusion") => get_inclusion(stream, store_dir, &req),
        ("GET", "/proof/consistency") => get_consistency(stream, store_dir, &req),
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

/// Leaves of the durable log, derived the way `verify::against_root` derives
/// them. Re-opening from disk is the point: a proof must not cover a record
/// that is still in flight.
fn durable_leaves(store_dir: &Path) -> Result<(Vec<[u8; 32]>, usize), String> {
    let store = Store::open(store_dir).map_err(|e| e.to_string())?;
    let mut leaves = Vec::with_capacity(store.records().len());
    for r in store.records() {
        leaves.push(leaf_hash(&digest(r).map_err(|e| e.to_string())?));
    }
    let n = leaves.len();
    Ok((leaves, n))
}

/// `GET /root` — the same renderer the CLI prints, over a store re-opened
/// from disk so the signed root can only ever cover durable records.
fn get_root(stream: &UnixStream, store_dir: &Path) {
    let store = match Store::open(store_dir) {
        Ok(s) => s,
        Err(e) => return server_error(stream, &e.to_string()),
    };
    let accepted = match against_root(store.records(), None) {
        Ok(a) => a,
        Err(e) => return server_error(stream, &e.to_string()),
    };
    let signed = SoftwareSigner::generate(DEV_SEED).sign_root(&accepted.root, accepted.size);
    // The SAME renderer the CLI prints. A second formatter would drift, and
    // the text it would drift on is the sentence saying what this root does
    // not defend against.
    let body = format!(
        r#"{{"size":{},"root":{},"rendered":{}}}"#,
        accepted.size,
        json_string(&hex::encode(accepted.root)),
        json_string(&signed.render())
    );
    write_response(stream, 200, "OK", &body);
}

/// `GET /proof/inclusion?leaf=<m>&size=<n>`. Never fabricates a proof for a
/// leaf or size the durable log does not have — the `size > n` guard below
/// runs before the slice it protects.
fn get_inclusion(stream: &UnixStream, store_dir: &Path, req: &Request) {
    let (Some(leaf), Some(size)) = (req.u64_param("leaf"), req.u64_param("size")) else {
        return bad_request(
            stream,
            "both `leaf` and `size` are required and must be integers",
        );
    };
    let (leaves, n) = match durable_leaves(store_dir) {
        Ok(v) => v,
        Err(e) => return server_error(stream, &e),
    };
    if size as usize > n {
        return not_found(stream, "the log is smaller than the requested size");
    }
    let prefix = &leaves[..size as usize];
    let Some(path) = inclusion_proof(prefix, leaf as usize) else {
        return not_found(stream, "no such leaf in a tree of that size");
    };
    let elems: Vec<String> = path.iter().map(hex::encode).collect();
    let body = format!(
        r#"{{"leaf":{leaf},"size":{size},"root":{},"path":[{}]}}"#,
        json_string(&hex::encode(root_of(prefix))),
        elems
            .iter()
            .map(|e| json_string(e))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_response(stream, 200, "OK", &body);
}

/// `GET /proof/consistency?from=<m>&to=<n>`. Never fabricates a proof for a
/// range the durable log does not have — the `to > n` guard below runs before
/// the slice it protects, and `from == 0 || from > to` is refused before any
/// store access at all.
fn get_consistency(stream: &UnixStream, store_dir: &Path, req: &Request) {
    let (Some(from), Some(to)) = (req.u64_param("from"), req.u64_param("to")) else {
        return bad_request(
            stream,
            "both `from` and `to` are required and must be integers",
        );
    };
    if from == 0 || from > to {
        return bad_request(stream, "`from` must be non-zero and no greater than `to`");
    }
    let (leaves, n) = match durable_leaves(store_dir) {
        Ok(v) => v,
        Err(e) => return server_error(stream, &e),
    };
    if to as usize > n {
        return not_found(stream, "the log is smaller than `to`");
    }
    let prefix = &leaves[..to as usize];
    let Some(proof) = consistency_proof(prefix, from as usize) else {
        return not_found(stream, "no consistency proof exists for that range");
    };
    let elems: Vec<String> = proof.iter().map(hex::encode).collect();
    let body = format!(
        r#"{{"from":{from},"to":{to},"old_root":{},"new_root":{},"proof":[{}]}}"#,
        json_string(&hex::encode(root_of(&leaves[..from as usize]))),
        json_string(&hex::encode(root_of(prefix))),
        elems
            .iter()
            .map(|e| json_string(e))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_response(stream, 200, "OK", &body);
}

fn bad_request(stream: &UnixStream, why: &str) {
    write_response(
        stream,
        400,
        "Bad Request",
        &format!(r#"{{"error":{}}}"#, json_string(why)),
    );
}

fn not_found(stream: &UnixStream, why: &str) {
    write_response(
        stream,
        404,
        "Not Found",
        &format!(r#"{{"error":{}}}"#, json_string(why)),
    );
}

fn server_error(stream: &UnixStream, why: &str) {
    write_response(
        stream,
        500,
        "Internal Server Error",
        &format!(r#"{{"error":{}}}"#, json_string(why)),
    );
}

/// Minimal JSON string escaping, so an error message cannot break the body it
/// is embedded in. `serde_json` does this properly and is used for every real
/// payload; this exists only for the error path, which must never itself fail.
pub(crate) fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
