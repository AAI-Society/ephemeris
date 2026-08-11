# Measurements

Produced by `cargo run --release --features bench --bin ephemeris-bench`.

**The `throughput` and `latency` arrays are measurements of one machine at one
moment, not properties of `ephemeris`.** Every file records the host that
produced it — CPU, core count, OS, the sync call used, and the measured cost
of that call. Read those fields before reading any throughput number.

**The `linkability` arrays are the exact inverse of that, despite sitting in
the same JSON object under the same `host` key.** Nothing in them was timed.
`src/bin/ephemeris-bench.rs` builds the per-agent action list handed to
`linkability::measure` from `--agents` and `--claims-per-agent` alone; it
never reads the `RunReport` the throughput run just produced. So every
linkability figure is a closed-form function of the tree-assignment function
`tree_for` and those two numbers: byte-identical on any host, and byte-identical
even if every one of the 20,000 claims in the throughput run had been refused.
They *are* properties of `ephemeris` and are *not* properties of this machine.
The `host` block applies to the throughput and latency arrays; it does not
qualify the linkability ones, and re-running on other hardware would not
change a digit of them. See section 4.

**Do not compare across platforms without checking `sync_call` and `sync_us`.**
On macOS `File::sync_data` is `F_FULLFSYNC` and forces a drive cache flush,
costing milliseconds. On Linux it is `fdatasync`, which on typical hardware does
not. A Linux run may look an order of magnitude faster while providing *weaker*
durability, so the comparison is meaningless unless both figures are quoted.

`prediction` in each file is what the design expected **before** anything was
measured. It is recorded so a contradicting result reads as a contradiction
rather than quietly becoming the new expectation.

## Outcome

The prediction recorded in each file was **not confirmed**. Read this section
before quoting any number below — several read very differently once the
harness's actual power (and lack of it) is accounted for.

### 1. The throughput half of the prediction failed

The prediction was `claims_per_sec ≈ mean_batch_size * (1e6 / sync_us)`. Using
the `global` file's rows (agents = 1, 10, 100, 1000), the ratio of actual to
predicted is:

(predicted = `mean batch × 1e6/3438.2575`, i.e. × 290.845 calls/s, carried
at full precision rather than from the rounded 290.9)

| agents | mean batch | predicted claims/s | actual claims/s | actual / predicted |
|---|---|---|---|---|
| 1 | 1.0 | 290.8 | 274.1 | **0.94** |
| 10 | 7.14 | 2077.5 | 1734.4 | **0.83** |
| 100 | 90.9 | 26,440.5 | 10,929.7 | **0.41** |
| 1000 | 909.1 | 264,404 | 34,566.1 | **0.13** |

The `per-agent` and `joint` files show the same pattern at the same agent
counts, within the run-to-run noise quantified in section 3. The prediction
holds at batch size 1 and fails by roughly 7x at batch size 909.

The per-batch, non-fsync time (total wall time per commit cycle, minus the
measured `sync_us`) rises from about **0.21 ms at batch size 1** to about
**22.9 ms at batch size 909**. The commit loop (`run_loop` in
`src/commit.rs`) is a single thread, and everything it does per record
happens serially in that thread before the next `fsync` can start:

- a **full deep clone of every claim in the batch**, before any record is
  built at all — `run_loop` builds its `pairs` vector with
  `batch.iter().map(|s| (s.claim.clone(), s.now_secs))`, copying a ten-field
  struct with nine owned `String`s, 909 times over at the top of the sweep,
  allocating each one;
- building every `Record`: canonicalization, SHA-256, hash-chain extension;
- and, once `append_batch` returns, waking up to 909 blocked submitter
  threads.

That is consistent with the residual growing roughly with batch size and
dominating wall time at the top of the sweep. The clone is listed first
because it is the cheapest to remove and was the easiest to overlook: it is
avoidable — after this line the batch's claims are never read again, only its
reply channels are, so the submissions could be split into claims and repliers
instead of copied — but removing it would take an allocation off the
measured path and invalidate every number in this directory, so it is
reported here rather than fixed under the same change that published these
figures.

### 2. Caveat on the number above: `sync_us` itself is an underestimate at scale

`measure_sync_us` in `src/bench/host.rs` probes `write` + `sync_data` on an
**11-byte** write, repeated 50 times. A real batch of 909 records writes
hundreds of kilobytes in one `write` + `sync_data` pair, and `F_FULLFSYNC`'s
cost is not independent of how many dirty bytes it has to flush. So
`mean_batch_size * (1e6/sync_us)` over-predicts at large batch size **by
construction** — the probe's `sync_us` is a lower bound on what a large
batch's real sync actually costs, not a fixed per-call constant.

Concretely: "fsync accounts for ~13% of wall time at 1000 agents" (22
fsyncs × 3438 us ≈ 75.6 ms against 578.6 ms wall) is an **upper bound on the
non-fsync residual**, not a decomposition of wall time into an fsync part and
a non-fsync part. Some of the other 87% may be real sync cost that the
11-byte probe did not capture. Section 1's "serial per-record work
dominates" explanation should be read as the best available account, not as
an attributed percentage split.

### 3. The topology half of the prediction was never tested

`cfg.topology` is used in exactly one place in the throughput harness —
`src/bench/run.rs`, to label the output (`cfg.topology.name().to_string()`).
No tree is built or maintained on the write path: `src/commit.rs` and
`src/store.rs` do no `Topology`/`Forest`/`tree_for` work at all. So the three
`throughput` arrays in `results/{global,per-agent,joint}.json` are three
repetitions of **the same experiment**, relabelled — the harness has no
mechanism by which a real topology effect on throughput could show up.

**We make no claim that topology does not affect throughput. We did not test
it.** The three runs are still useful, but only as a run-to-run noise
estimate for identical code, not as a topology comparison:

| agents | global | per-agent | joint | spread (max−min ÷ mean) |
|---|---|---|---|---|
| 1 | 274.1 | 286.1 | 281.5 | 4.3% |
| 10 | 1734.4 | 1287.7 | 1401.8 | 30.3% |
| 100 | 10,929.7 | 12,846.1 | 13,375.1 | 19.7% |
| 1000 | 34,566.1 | 35,109.3 | 35,248.7 | 2.0% |

Run-to-run noise on this single host, single run, ranges from about 2% to
about 30%, depending on agent count — quoting the tightest row (1000 agents,
2%) as "the" measurement precision would be cherry-picking. A real topology
throughput comparison needs the harness to actually build the forest (e.g.
by having the committer construct `Record`s per-tree, or by measuring
`Forest::insert` cost directly), which this task does not do.

### 4. Joint vs. per-agent linkability is a tautology; global vs. per-agent is real, and derived rather than measured

`Joint::tree_for` in `src/topology.rs` is byte-for-byte identical to
`PerAgent::tree_for` — both return `TreeId(format!("{AGENT_PREFIX}{agent}"))`.
The only place `Joint` differs from `PerAgent` is `anchor_policy()`, which
carries `interval_secs` and which `linkability::measure` never calls (it only
calls `kind.tree_for(agent)`). So the linkability metric **cannot
distinguish `Joint` from `PerAgent` at any interval or any run length** —
this is not a property of the 60-second interval used in this run or of how
long the run took; a thousand-year run with a one-second interval would
produce identical numbers, because `tree_for` takes no time argument at all.
The `joint.json`/`per-agent.json` linkability arrays matching exactly is
therefore expected by construction, not a result.

**Global vs. per-agent is a real result about `tree_for` — derived, not
measured**: `global`'s `effective_anonymity_set` equals the full action count
at every row (one tree, `entropy_bits = 0`), while `per-agent`'s is 20.0 at
every row (each agent indistinguishable only from itself). That comparison
genuinely exercises `tree_for`, the divergence is real, and it is the finding
this directory is for. But it was *derived*, not timed: as the header says,
`measure` is fed a list built from `--agents` and `--claims-per-agent`, so
every one of these numbers is analytic in (agents × claims-per-agent),
identical on any host, and unchanged had the run acked nothing. Nothing about
the gap depends on this machine, this run, or on any byte having reached
disk. "We measured linkability" is the wrong phrase; anyone with
`src/topology.rs` and a pencil reproduces these columns exactly.

One artefact of the derivation to flag, because it is easy to over-read. The
flat **20.0** in the `per-agent` column is the *stipulated uniform workload*,
not an observed distribution: the harness gives every agent exactly
`--claims-per-agent` actions. `effective_anonymity_set` is `sum(n_i^2) / N`,
the size-weighted mean of squared per-tree action counts, which equals one
agent's own action count only when every agent contributed the same number —
see the field's doc comment in `src/bench/linkability.rs`. Real fleets are not
uniformly active, so a real per-agent forest's figure would be that
size-weighted mean, pulled above the arithmetic mean by the busiest agents.
The direction and unboundedness of the global-vs-per-agent gap do not depend
on uniformity; the particular value 20.0 does.

Measuring `Joint`'s actual anchoring benefit over `PerAgent` — the coarser
cross-agent ordering `anchor_policy` is supposed to buy — would need a
metric that reads ordering/anchoring, not just tree membership. That metric
does not exist yet; it is future work, not something this run measured.

### 5. The latency figures at high concurrency are closed-loop, not service time

Every agent thread submits with zero think time and blocks on its `Ack`, so
offered load is unbounded and the system is saturated by construction once
more than a handful of agents are running. Little's law confirms this
directly: at 1000 agents (global), `claims_per_sec × p50_latency ≈ 34,566.1
× 0.027127 s ≈ 937` requests in flight — essentially the full agent count.
The reported `p99 = 52.3 ms` at 1000 agents is therefore **concurrency
divided by throughput** (queueing delay under saturation), not the log's
per-record service time.

The unloaded figure is the **1-agent row**: `p99 = 4.16 ms` (global),
essentially one `fsync` with nothing queued behind it. An architect sizing a
gateway's fail-closed logging budget from this file should use the 1-agent
row for service time and treat the 1000-agent row as "how bad it gets under
saturation with unbounded offered load," not as a latency SLO to budget
against directly.

### 6. Throughput excludes Merkle tree maintenance entirely

Per section 3, the commit path measured here builds no tree at all — no
`Forest::insert`, no root recomputation. A deployment that maintains the
forest incrementally alongside every commit would be slower than every
number in this file by an amount this run did not measure.

### 7. Provenance and reproduction

`crate_commit` in every JSON file reads `25e56f1` — the library state that
was actually exercised — but that commit's tree does not contain
`ephemeris-bench` itself; the binary was added in `d603168`. To reproduce:
check out `d603168` (or later) to get the binary, which measures whatever
library code is present at run time; `25e56f1` names the code that produced
*this* run's numbers.

This is a single run per cell, on a single host, with no warm-up and no
repetitions — see section 3 for how much that alone can move a number (up to
~30% between three back-to-back runs of identical code). Do not treat any
single figure here as more precise than that.
