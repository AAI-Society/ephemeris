<p align="center">
  <img src="assets/banner.svg" alt="ephemeris — internal consistency is not evidence" width="100%">
</p>

<p align="center">
  <img alt="Rust 2021" src="https://img.shields.io/badge/rust-2021%20%C2%B7%201.90%2B-CFFF04?style=flat-square&labelColor=0A0A0A">
  <img alt="tests passing" src="https://img.shields.io/badge/tests-54%20passing-CFFF04?style=flat-square&labelColor=0A0A0A">
  <img alt="Apache 2.0" src="https://img.shields.io/badge/licence-Apache--2.0-CFFF04?style=flat-square&labelColor=0A0A0A">
  <img alt="Phase 1 of 2 plus bench" src="https://img.shields.io/badge/build-phase%201%20of%202%20%2B%20bench-8F8A82?style=flat-square&labelColor=0A0A0A">
</p>

---

## "The log verifies"

It does. That is the problem.

A tamper-evident log gives you a hash chain and a signed root. Every record
commits to its predecessor, the root commits to all of them, and the signature
commits to the root. If anybody alters a past record, a link breaks and the
verifier says so. That is the whole pitch, and every part of it is true.

Now consider who holds the signing key.

An operator who wants to change what happened does not break a link. They
alter the record, recompute every link after it, re-sign the new root, and
hand you a log with no defect of any kind. Watch it happen:

```console
$ cargo run --example rewrite

Monday — root published at size 8:
  167c4adab3871ae8cc3d8f8b43c2aa8b47f983c969d9091105233b538be80616

Tuesday — record 3's verdict is rewritten and the chain relinked.

  chain verifies internally:      true
  step indices contiguous:        true
  accepted with no prior root:    true
  root signs cleanly:             true
  signature:                      518379ef066ed5b3fbf27a1c6d9df06a…

Nothing is broken. There is no failing check to notice, no torn
record, no gap in the sequence. A verifier handed this log alone
has nothing to object to, and it is a different log than the one
those eight actions produced.

Against Monday's published root:

  REFUSED — the log is not an append-only extension of the root published
  at size 8; a record before that point was altered and the chain relinked

The defence is not a better checksum. It is that somebody else
was holding a copy before the rewrite happened.
```

Every check the log can run **on itself** passes, because the rewritten log is
a perfectly good log — just of different events. **Internal consistency is a
property of the artefact, and tampering is a property of history.** No amount
of checking the artefact recovers the history.

The one line that refuses is the one comparing against a root that left the
operator's hands on Monday. That is not a cryptographic improvement. It is a
custody arrangement, and it is the only thing here doing real work.

---

## What ephemeris does

An append-only evidence log for Proof-of-Control records. It accepts claims
from an enforcement point, chains them, builds an RFC 6962 tree over them,
signs roots, serves inclusion and consistency proofs, and **refuses to
acknowledge a claim it has not durably written**.

It is deliberately unbalanced in what it emphasises. The proof code is
exhaustively verified and the durability path is built so the mistake cannot
be expressed. The signer is a labelled placeholder. That is the right split
for Phase 1, because the proofs and the durability are what a later TEE signer
would sit on top of, and a TEE signer over a wrong Merkle implementation is a
hardware-attested wrong answer.

---

## Two results we did not expect

### The durability test that proves nothing

The requirement is fail-closed: no claim is acknowledged before its bytes are
durable. The natural way to test it is fault injection — make the evidence
store unavailable and assert that every claim is refused.

The natural way to make it unavailable is to delete the file.

That test passes while the system is broken. A process holding an open file
descriptor keeps writing successfully after its path is unlinked; the writes
land in an inode with no directory entry, `fsync` reports success, and the
committer acknowledges claims backed by bytes that nothing can ever read
back. The durability check is satisfied in the narrowest possible sense and
the evidence is gone.

```
write to unlinked fd: Ok(())
reopen at same path:  Err(Os { code: 21, kind: IsADirectory })
```

So the fault injection injected no fault, and the assertion it was protecting
never ran. We found this because the assertion failed for the *opposite*
reason — the claims were acked, not refused — which is the only reason we
looked at the mechanism at all. A version of this test that had been written
slightly differently would have passed forever.

The fix is not in the test. A committer that cannot re-open its store now
refuses, because a descriptor outliving its path is exactly the condition
under which acking is unsafe.

### The chain the schema specifies does not cover the record

The evidence schema defines `chain_head` as
`H(previous_head ‖ canonical_snapshot_hash ‖ verdict)` — two fields of the
record, plus the previous head. So the chain protects those two fields and
nothing else. `step_index`, `target_resource`, `policy_bundle_hash`, and every
identity field can be altered without breaking a single link, and the log will
verify.

This crate chains over the whole record digest instead. It is strictly
stronger, and it is free: the digest is already computed for the Merkle leaf.
We report this upstream rather than honour it.

---

## Try it

```bash
cargo run --example rewrite        # attack A9, end to end
cargo test                         # 54 tests, all offline
cargo test --features bench        # 61, including the harness's own tests

cargo run --release --features bench --bin ephemeris-bench   # regenerate results/
```

The proof verifiers are checked exhaustively rather than by sampling: every
leaf of every tree and every prefix of every extension, for all sizes from 1
to 33. Merkle bugs hide at the sizes that are not powers of two, and a spot
check at 8 and 16 finds nothing.

---

## The three commands

```
ephemeris append --dir ./log --claim ./claim.json --now 1700000000
ephemeris root   --dir ./log
ephemeris verify --dir ./log [--prior-size N --prior-root <hex>]
```

Time is always injected via `--now` and never read from the system clock.

Exit codes: `0` success, `1` a verification or policy failure, `2` bad input
or configuration.

Every root this tool prints carries the sentence saying what it does not
defend against. That is not a disclaimer bolted on at the edge; `render()` is
the only way to print a root and it cannot produce one without it.

---

## What is real, and what is not

This project is about not overclaiming, so:

**Real.** The hash chain, the RFC 6962 tree with leaf/node domain separation,
inclusion proofs, consistency proofs, durability, group commit, and crash
recovery. The proof verifiers are checked exhaustively to 33 leaves against a
reference recursive implementation, not against themselves. A torn trailing
record is truncated; a *complete* record whose trailing digest does not match
its bytes is refused rather than skipped, because silently dropping evidence
is the failure this tool exists to prevent.

**A labelled placeholder.** The signer is a process-local software key. It
provides integrity against anybody who is not the operator and **no defence
against the operator**. `Custody::Tee` exists in the type and is constructed
nowhere in this build, so a default build cannot emit a root claiming hardware
custody. The TEE signer is Phase 2.

**Not defended.** C7.3.3 equivocation. No root is published outside the
operator's control, so **attack A9 stands** — the demonstration at the top of
this page is not a historical note about a bug we fixed, it is the current
state of the tool. `verify` without `--prior-root` runs only the internal
checks, and a rewritten log passes them. That is not an oversight to be
tightened later; it is the honest shape of a component that publishes nothing
externally. Making it `Option` rather than defaulting it is how the code says
so.

**Absent, not stubbed.** `path_summary_hash` is carried as `null` because no
tool computes a bounded path summary; a record carries `null` rather than a
fabricated digest. `agbom_digest` has no field and no API, because no AgBOM
tooling exists and a stub would be read as a measurement. See P04, P10, and
`spectrum`.

**Measured, on one machine, once.** Phase 2's `bench` ran 1, 10, 100 and 1,000
agents against each topology. The host was an Apple M5 Max, 18 logical cores,
macOS, where `File::sync_data` is `F_FULLFSYNC` and costs a measured 3,438 µs.
One run per cell, no warm-up, no repetitions. That framing covers the
throughput and latency numbers below; the linkability numbers are a different
kind of thing and the next paragraph says so. Everything below is in
[`results/`](results/README.md), and **that file's "Outcome" section is the
authoritative reading** — it lists caveats this summary compresses.

*Linkability is the half we got an answer for — derived rather than timed,
and it is where the objection lands.* The harness computes these figures from
the agent count and the per-agent claim count alone, feeding them straight to
the tree-assignment function; it never reads the throughput run's output. So
they are a closed-form property of `ephemeris`, identical on any host and
unchanged had the run acknowledged nothing — the opposite of the "one machine,
once" caveat above, despite living in the same JSON files. A single
global tree's effective anonymity set grows with the fleet: 20, 200, 2,000,
20,000 actions at 1/10/100/1,000 agents, one tree, 0.00 bits of tree-identity
entropy at every size. A tree per agent pins the effective anonymity set at
**20.0 at every fleet size**, with entropy rising 0.00 → 3.32 → 6.64 → 9.97
bits — exactly log₂(agents). So the gap between the two arrangements widens
without bound as the fleet grows. The flat 20.0 is the stipulated uniform
workload, not an observed distribution: the effective set is a size-weighted
mean and collapses to one agent's own action count only when every agent acts
equally often. Real fleets are not uniformly active — which changes that
number, not the direction of the gap. Joint's figures equal per-agent's by
construction, because `Joint::tree_for` is byte-identical to
`PerAgent::tree_for` and the metric never reads the anchor policy; global
versus per-agent is the only real comparison here, and joint's anchoring
benefit remains unmeasured.

*Unloaded latency is one fsync.* At one agent, p50 3.9 ms and p99 4.2 ms. The
p99 of 52.3 ms at 1,000 agents is a **closed-loop saturation** figure — agents
submit with zero think time, so offered load is unbounded by construction, and
Little's law reproduces the number as concurrency ÷ throughput. Size a
fail-closed logging budget from the 1-agent row.

*The throughput prediction failed.* We predicted `mean_batch_size × (1e6 /
sync_us)` before measuring. Actual ÷ predicted is 0.94 at one agent and 0.13
at a thousand: it holds at batch size 1 and misses by roughly 7× at batch size
909. Group commit does amortise — 274 claims/s at one agent, 34,566/s at a
thousand — but throughput stops tracking the sync ceiling well before reaching
it. Per-batch non-sync time rises from ~0.21 ms at batch size 1 to ~22.9 ms at
batch size 909; our best account of that is the single commit thread, which
canonicalizes, hashes and chains every record serially before the batch's one
`fsync` can start. That is an account, not an attribution. Treat the implied **non-sync
residual as an upper bound**, not as a decomposition of wall time:
`sync_us` was probed with an 11-byte write, while a full batch writes
hundreds of kilobytes, and `F_FULLFSYNC`'s cost is not independent of how
many dirty bytes it flushes — so some of what looks like non-sync time may
be real sync cost the probe never saw.

*The topology axis was not tested.* `run()` uses the topology only to label
its output; the commit path does no tree work at all, so the three throughput
sweeps are one experiment repeated three times. We use them only as a
run-to-run noise estimate — spreads of 4.3%, 30.3%, 19.7% and 2.0% at
1/10/100/1,000 agents. **We make no claim that topology does not affect
throughput; we did not test it.** These numbers also exclude Merkle tree
maintenance entirely, so a deployment maintaining the forest incrementally
would be slower by an amount this run did not measure. Do not compare any of
this to a Linux number without quoting the sync call: `fdatasync` typically
does not flush the drive cache, so Linux would look faster while being
*weaker*.

**The limit that matters.** Everything this tool does well is downstream of a
custody arrangement it does not implement. If nobody outside the operator ever
holds a root, the exhaustive proof verification buys you nothing against the
one adversary who matters. If you take one thing from this repository, take
that rather than the proof code.

---

## How this maps to the standard

`ephemeris` is a research tool for the
[**Proof-of-Control Standard**](https://github.com/AAI-Society/ov-poc-standard).

| | Standard | |
| :-- | :-- | :-- |
| ⚠️ | **`chain_head`** — defined as `H(previous_head ‖ canonical_snapshot_hash ‖ verdict)` | two record fields, not the record. `step_index`, `target_resource`, `policy_bundle_hash` and the identity fields are outside the chain and can be altered without breaking a link |
| ⚠️ | **C7.3.5** — the reference implementation compares record counts | length is not evidence. A rewritten log has exactly the right length; the test asserting so is `a_step_count_comparison_alone_would_have_accepted_the_rewrite` |
| ◻︎ | **C7.3.3 equivocation** — no external publication point is specified | until one exists, A9 stands against every deployment of this component, however well its proofs are implemented |
| ◻︎ | **C7.1.3 fail-closed** — stated as a requirement, with no test methodology | the obvious fault injection does not inject the fault; see above |

⚠️ is a contradiction; ◻︎ is a gap this tool surfaces and does not close.

---

## The paper

[**Internal Consistency Is Not Evidence: Custody, Durability, and the Limits
of a Tamper-Evident Log**](paper/main.pdf) — readable in the browser, no
toolchain needed.

**Draft, circulated for co-author review.** It argues that the security of a
tamper-evident log is bounded by a custody arrangement rather than by its
cryptography, works attack A9 all the way through, and reports the two
findings above — including the durability test that passes while the system it
tests is broken, which we think is the more useful of the two because it is a
statement about how the property gets *tested* rather than about one schema.

Its figures come from `results/*.json` — throughput and latency on one
machine, one run per cell; linkability derived from the tree-assignment
function and so not any machine's — and it reports the mistake that produced
its most interesting finding rather than presenting the finding as foresight.

---

## Related

Named for the observational-astronomy method each one uses:

- **[parallax](https://github.com/Task-force-for-AI-agents-in-Healthcare/parallax)** — depth from two viewpoints. Computes the residual trust set of an attestation deployment.
- **[transit](https://github.com/Task-force-for-AI-agents-in-Healthcare/transit)** — infer a hidden body from what it does to passing light. Signing the bytes is not signing the deed.
- **[occultation](https://github.com/Task-force-for-AI-agents-in-Healthcare/occultation)** — one body deliberately hiding another. Measures what unlinkability costs at agent action rates.
- **ephemeris** — positions tabulated at successive epochs, so a later claim can be checked against an earlier record. This one.
- **[spectrum](https://github.com/Task-force-for-AI-agents-in-Healthcare/spectrum)** — decompose the light to find what a thing is made of. A running commitment to agent composition.

`ephemeris` depends on `transit` for RFC 8785 canonicalization rather than
carrying a second canonicalizer. Two implementations that serialize the same
record differently produce different digests, every signature still verifies,
nothing looks broken, and the natural repair is to relax the comparison until
it works — which removes the property the comparison existed to provide.

---

## Building

`transit` and `occultation` are **private** sibling repos in this org, and you
need read access to **both**, even for a default build. `occultation` is only
compiled under `--features bench`, but cargo resolves optional dependencies
regardless of features, so its git source must be fetchable for any build —
without it, a plain `cargo build` fails with ``failed to load source for
dependency `occultation` ``. Compilation is feature-gated; resolution is not.
Concretely: a contributor with access to `transit` but not `occultation` could
build this crate before this branch and cannot after it.

`.cargo/config.toml` sets `net.git-fetch-with-cli = true`; do not delete it,
for the reasons documented in that file.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# The bench feature, which CI also gates on. Feature-gated code that nothing
# builds stops compiling quietly.
cargo clippy --all-targets --features bench -- -D warnings
cargo test --features bench
```

Everything runs offline: no network, no credentials, no TEE.

---

## Phase 2, deliberately not here

1. The UDS listener and its routes, so `transit guard` can write to it.
2. The TEE signer behind a cargo feature, with `Custody::Tee` reachable only
   from it, and a test that a default build cannot claim hardware custody.
3. The `poc-audit` cross-check, which needs a released tag of this repo.
4. A throughput harness that actually builds the forest, so the topology axis
   can be measured rather than labelled — and a linkability metric that reads
   ordering, so joint's anchoring can be told apart from per-agent's.

## Licence

Apache-2.0 throughout, code and paper alike. Built for the
[Advanced AI Society](https://advancedaisociety.org) Proof-of-Control initiative —
see [**AAI-Society/ov-poc-standard**](https://github.com/AAI-Society/ov-poc-standard).

<p align="center"><img src="assets/logo.svg" width="52" alt=""></p>
