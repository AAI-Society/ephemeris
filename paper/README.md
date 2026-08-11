# Internal Consistency Is Not Evidence: arXiv Preprint

LaTeX source for *Internal Consistency Is Not Evidence: Custody, Durability,
and the Limits of a Tamper-Evident Log*.

**Status: working draft for co-author review — not yet submitted.** All
sections are written; there is no placeholder section. §7 ("What It Costs, and
What We Got Wrong") reports the Phase 2 measurements, and §8 ("What We Still
Don't Know") states what remains open after them — including the fact that the
throughput half of the experiment never tested the topology axis at all.

§8 now also reports that the implementation carries a TEE signer behind a
compile-time feature. **That paragraph reports existence, not results.** No
part of this work has run inside a confidential-computing guest; only the
signer's failure paths are exercised, offline. Nothing was measured, and the
section must not read as though anything was. See required step 5 below.

**Prose style.** The expository register shared with the sibling papers: first
person, concrete case before abstraction, ordinary words in place of jargon,
and ignorance stated loudly rather than hedged. Specifically —

- **The attack is demonstrated before it is formalised.** §1 walks through the
  rewrite in English and §3 opens by admitting the symbols "will not make
  anything truer than the English already did."
- **The central claim is stated in its blunt form first**, then qualified. The
  claim that cryptographic quality is *independent* of operator-resistance is
  meant to be startling, and §1 defends the word "independent" rather than
  softening it.
- **The mistake is reported as a mistake.** §4.2 exists to say that the best
  finding in the paper was luck: the broken fault injection was caught only
  because an unrelated bug in the same test failed loudly first. The tidy
  version of that story would be dishonest, and the section says so.
- **The biconditional is flagged as half-argued.** §3 states the custody
  condition as "if and only if" and then immediately says which direction is
  actually defensible and which smuggles in an organisational assumption.

Section titles stay plain declaratives ("The Problem", "The Test That Proves
Nothing", "What We Still Don't Know").

**The ordering is load-bearing.** §1 opens with the rewrite rather than with
the durability finding, because the rewrite is what makes the custody argument
unavoidable, and the durability finding is a second, separable result. Do not
promote §4 to the front in editing: it is the more novel finding but the less
structural one, and leading with it makes the paper look like a bug report.

**Where the numbers come from.** Every figure in §7 is read out of
`../results/*.json`, produced by
`cargo run --release --features bench --bin ephemeris-bench`. The
**throughput and latency** figures are **one machine's** — an Apple M5 Max, 18
logical cores, macOS, `File::sync_data` = `F_FULLFSYNC` at a measured 3438 µs —
**one run per cell, no warm-up, no repetitions**. The library state measured is
commit `25e56f1`; the bench binary that reproduces it was added in `d603168`.

**The linkability figures are not measurements and are not this machine's.**
The bench binary hands `linkability::measure` an agent list built from
`--agents` and `--claims-per-agent` and never reads the throughput run's
report, so those figures are a closed-form function of `tree_for`: identical on
any host, and identical had the run acked nothing. §7.1 must describe them as
derived, never as measured, and must not let the "one machine, one afternoon"
framing that correctly qualifies §7.2–§7.4 spill onto them — that framing is
exactly inverted for §7.1.

Three things the section is required to keep saying, because a review caught
each of them being got wrong the first time:

- **The topology axis is declared untested.** The commit path builds no tree,
  so the three throughput sweeps are one experiment repeated three times. They
  are used only as a run-to-run noise estimate (4.3%–30.3% spread). The paper
  must never say "topology has little effect on throughput".
- **Joint vs. per-agent linkability is a tautology**, not a result:
  `Joint::tree_for` is byte-identical to `PerAgent::tree_for` and the metric
  never reads the anchor policy. Global vs. per-agent is the real comparison.
- **The linkability result is derived, not measured**, per the paragraph
  above — and the flat per-agent 20.0 is the stipulated uniform workload, not
  an observed distribution. If an edit reintroduces "we measure, on one
  machine, what the tree arrangement costs in linkability", revert it.

`results/README.md` is the authoritative reading of these numbers. If §7 and
that file ever disagree, that file wins.

## Build

```bash
cd paper
tectonic main.tex
```

Output: `main.pdf`, which is committed to the repository so it can be read
without building anything.

**It is not diff-gated in CI, and that is deliberate.** Tectonic stamps a
`CreationDate` into the PDF, so two builds of identical sources differ
byte-for-byte and a `git diff --exit-code` check would fail on every run. CI
builds the paper to prove it still compiles; keeping the committed PDF current
is a manual step. If you change `main.tex`, rebuild and commit the PDF in the
same commit.

`-Z shell-escape` is *not* required. This preamble omits `minted` and uses
`listings`, so the CI build has no Python dependency.

## Before Submission — Required Steps

1. **Co-author consent.** The author list is deliberately short at this stage.
   Add co-authors only on explicit consent and substantive contribution; the
   title-page footnote states this.

2. **Citation verification.** Every entry in `references.bib` must be
   confirmed against the primary source before submission. **None has been
   confirmed yet.** Check with particular care: the page range on Merkle
   (1980), the volume and page range on Haber & Stornetta (1991), the page
   ranges on both OSDI 2014 entries, and the full author list on ARIES
   (1992) and Pillai (2014). The Crosby & Wallach and Rebello entries have no
   page numbers recorded and need them.

3. **The POSIX claim.** §4 asserts that writes to an unlinked descriptor
   succeed and that this is specified rather than incidental. The behaviour is
   demonstrated on macOS in `../examples` territory and by the probe quoted in
   the section. Before submission, either cite the specific POSIX clause for
   `unlink` retaining the file until the last descriptor closes, or downgrade
   the claim to "on the systems we tested". Do not leave it asserted as
   universal on the strength of one platform.

4. **The generalisation in §8.** The paper says it "thinks" the broken
   fault-injection pattern is common and explicitly labels that a hypothesis.
   Keep it labelled. If anyone surveys other durability suites before
   submission, the result replaces the hypothesis; absent that, the hedge
   stays.

5. **The TEE signer in §8 has not been run on hardware.** The paper may say
   the code path exists, that it binds the key to a quote through the whole
   `SubjectPublicKeyInfo`, that it fails closed, and that its failure paths
   are tested offline. It may **not** say the signer works, that attestation
   is verified, that a quote was obtained, or that a platform accepted the
   `report_data`. None of those has happened. If an edit before submission
   introduces "we attest the signing key" or any sentence in which the TEE
   path is described as verified, revert it — and if somebody does run it in
   a real guest before submission, that is a new result needing its own
   description of the platform, not a tense change.

   The same paragraph must keep saying that a TEE key changes *who can sign*
   and not *who publishes roots*, so attack A9 is untouched by it. This is the
   sentence most likely to be lost in editing, because the arrival of hardware
   custody reads like progress against the operator and is not.

6. **Re-derive every figure in §7 against `../results/*.json`.** The tables are
   typed into the `.tex`, not generated, so they can drift from the JSON
   silently. Before submission, re-check each cell of Tables 1 and 2, the
   latency figures, the 0.21 ms / 22.9 ms residuals, the 937-in-flight Little's
   law figure, and the four noise-spread percentages. Numbers that cannot be
   re-derived from the committed JSON must be cut, not rounded.

7. **Re-state the two things the numbers do not license.** §7 must still say
   that the topology axis was untested and that joint-vs-per-agent linkability
   is a tautology. If an edit softens either into "topology made little
   difference" or "joint's anchoring was evaluated", revert it.

8. **Repeat the run before submission, ideally on a second machine.** One host,
   one run per cell, with a self-reported noise floor of up to 30%, is thin for
   a published figure. Either add repetitions and a second platform (quoting
   its `sync_call` and `sync_us`), or keep §7's framing that these are one
   machine's numbers and not a benchmark.

9. **arXiv metadata.** Suggested categories: cs.CR (primary), cs.DC and cs.SE
   (secondary). Licence: Apache-2.0, matching the implementation.

## Files

| File | Purpose |
| --- | --- |
| `main.tex` | The paper (compiles with tectonic, XeTeX engine) |
| `references.bib` | Bibliography: RFCs, the Merkle and timestamping literature, the crash-consistency family |
| `main.pdf` | Compiled output. **Committed**, so it can be read in the browser without a LaTeX toolchain — regenerate and commit whenever `main.tex` changes |
