# Internal Consistency Is Not Evidence: arXiv Preprint

LaTeX source for *Internal Consistency Is Not Evidence: Custody, Durability,
and the Limits of a Tamper-Evident Log*.

**Status: working draft for co-author review — not yet submitted.** All
sections are written. Unlike the sibling `transit` paper, there is no
placeholder section, because this paper deliberately reports **no
measurements**: Phase 1 of `ephemeris` ships no benchmark, and §7 says so in
those terms rather than leaving a gap to be filled in later.

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

**No fabricated numbers.** This paper reports no throughput, latency, or
linkability figures, because none were measured. §7 states that explicitly.
If a future revision adds them, they must come from `bench` in Phase 2 and be
generated rather than typed.

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

4. **The generalisation in §7.** The paper says it "thinks" the broken
   fault-injection pattern is common and explicitly labels that a hypothesis.
   Keep it labelled. If anyone surveys other durability suites before
   submission, the result replaces the hypothesis; absent that, the hedge
   stays.

5. **arXiv metadata.** Suggested categories: cs.CR (primary), cs.DC and cs.SE
   (secondary). Licence: Apache-2.0, matching the implementation.

## Files

| File | Purpose |
| --- | --- |
| `main.tex` | The paper (compiles with tectonic, XeTeX engine) |
| `references.bib` | Bibliography: RFCs, the Merkle and timestamping literature, the crash-consistency family |
| `main.pdf` | Compiled output. **Committed**, so it can be read in the browser without a LaTeX toolchain — regenerate and commit whenever `main.tex` changes |
