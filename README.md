# ephemeris

An append-only evidence log for Proof-of-Control records. It accepts claims
from an enforcement point, chains them, builds an RFC 6962 tree over them,
signs roots, serves inclusion and consistency proofs, and **refuses to
acknowledge a claim it has not durably written**.

This is Phase 1.

## What is real and what is modelled

Read this section before trusting any output of this tool.

**Real.** These are implemented, tested, and mean what they say:

- The hash chain. Each record commits to the whole digest of its predecessor,
  not to three of its fields — see "specification gap" below.
- The RFC 6962 tree, with leaf/node domain separation. The incremental root is
  pinned against a reference recursive root at every size from 1 to 33.
- Inclusion proofs and consistency proofs, verified exhaustively over every
  leaf of every tree and every prefix of every extension up to 33 leaves.
- Durability. A record's bytes reach disk and `fsync` returns before any
  acknowledgement exists.
- Group commit. A batch shares one `fsync`; either all of its bytes are
  durable or none of them are acknowledged.
- Crash recovery. A trailing partial record is truncated; a *complete* record
  whose trailing digest does not match its bytes is refused, not skipped.

**Modelled, or absent.** These are the honest limits:

- **The signer is a process-local software key.** It provides integrity
  against anyone who is not the operator, and **no defence against the
  operator**. The TEE signer is Phase 2. `Custody::Tee` is never constructed
  in this build, so a default build cannot emit a root claiming hardware
  custody.
- **C7.3.3 equivocation is not defended.** No root is published outside the
  operator's control, so **attack A9 stands**. An operator holding the signing
  key can alter a past record, recompute every link, re-sign, and produce a log
  that replays perfectly. Only a root you obtained earlier and independently
  contradicts it. Every root this tool prints says so, in the output itself.
- `path_summary_hash` is carried as `null` because no tool computes a bounded
  path summary; see P04. A record carries `null` rather than a fabricated
  digest.
- `agbom_digest` is **absent**, not stubbed, because no AgBOM tooling exists;
  see P10 and `spectrum`. A stub would be mistaken for a real measurement.

## What A9 means in practice

A rewritten-and-relinked log is *internally perfect*. Every chain link
verifies, every step index is contiguous, the signature over its root is
valid. There is a test asserting exactly that, because the defence must not be
mistaken for the chain check.

The only thing that contradicts such a log is a root published **before** the
rewrite:

```
ephemeris verify --dir ./log --prior-size 8 --prior-root <hex>
```

Without `--prior-size`/`--prior-root`, `verify` runs only the internal checks,
which a rewritten log passes. That is not an oversight to be tightened later;
it is the honest shape of a tool that publishes no root outside the operator's
control.

## Specification gap

The evidence schema defines `chain_head` as
`H(previous_head ‖ canonical_snapshot_hash ‖ verdict)` — three fields of the
record, not the record. Chaining over only those three leaves every other
field unprotected: `step_index`, `target_resource`, `policy_bundle_hash` and
the identity fields could all be altered without breaking a single link.

This crate extends with the **whole record digest** instead. It is strictly
stronger and costs nothing, since the digest is already computed for the tree
leaf. The weaker schema definition is a finding to report upstream rather than
a constraint to honour.

## Usage

```
ephemeris append --dir ./log --claim ./claim.json --now 1700000000
ephemeris root   --dir ./log
ephemeris verify --dir ./log [--prior-size N --prior-root <hex>]
```

Time is always injected via `--now` and never read from the system clock.

Exit codes: `0` success, `1` a verification or policy failure, `2` bad input
or configuration.

## Building

`ephemeris` depends on `transit`, a **private** sibling repo in this org, for
RFC 8785 canonicalization — deliberately, rather than carrying a second
canonicalizer. Two implementations that serialize the same record differently
produce different digests, every signature still verifies, nothing looks
broken, and the natural repair is to relax the comparison until it works,
which removes the property the comparison existed to provide.

You therefore need read access to the org's repos. `.cargo/config.toml` sets
`net.git-fetch-with-cli = true`; do not delete it, for the reasons documented
in that file.

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Everything runs offline: no network, no credentials, no TEE.

## Phase 2, deliberately not here

1. The UDS listener and its routes, so `transit guard` can write to it.
2. The TEE signer behind a cargo feature, with `Custody::Tee` reachable only
   from it, and a test that a default build cannot claim hardware custody.
3. `bench` — throughput and tail latency against 1–1,000 agents, and
   linkability per topology.
4. The `poc-audit` cross-check, which needs a released tag of this repo.

## Licence

Apache-2.0. Copyright Advanced AI Society and the Proof-of-Control
contributors.
