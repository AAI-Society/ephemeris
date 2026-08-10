use crate::bench::TopologyKind;
use occultation::anonymity::{partition, AnonymityError, Fleet, Host as OccHost};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum LinkabilityError {
    #[error("a run with no actions has no anonymity to report")]
    NoActions,
    /// `measure` filters every agent with zero actions before building a
    /// fleet (see its doc comment), so `occultation::anonymity::partition`
    /// never sees an empty fleet, a zero-count host, or a host with an empty
    /// TCB map (this module always inserts exactly one `tcb` entry). Every
    /// `AnonymityError` variant — `Io`, `Parse`, `EmptyFleet`, `ZeroCount`,
    /// `EmptyTcb` — is therefore unreachable through `measure` today. This
    /// variant, and the `#[from]`, are kept only as a defensive backstop in
    /// case that invariant is ever broken by a future change; its `{0}`
    /// would then carry occultation's own wording (fleets, hosts, TCB) into
    /// an ephemeris-facing message, which is a latent violation of this
    /// module's one rule, not an active one — `no_error_message_measure_can_
    /// produce_leaks_occultation_vocabulary` below exercises every path
    /// `measure` can actually take and would fail the day this stops being
    /// true.
    #[error("the anonymity partition failed: {0}")]
    Partition(#[from] AnonymityError),
}

/// What a topology gives away, in ephemeris vocabulary.
///
/// The arithmetic comes from `occultation::anonymity::partition` — one
/// implementation of the metric, so the two tools stay comparable, which is the
/// same reason this crate reuses `transit::jcs` rather than writing a second
/// canonicalizer.
///
/// **The vocabulary does not come from there.** An `AnonymityReport` speaks of
/// fleets, hosts and TCB attributes; a reader finding one of those in
/// `results/` could reasonably take it for a hardware-attestation measurement.
/// Nothing in a committed artefact may read as a measurement of something it is
/// not, so the report is projected into these names and no `occultation` type
/// escapes this module.
#[derive(Clone, Debug, Serialize)]
pub struct LinkabilityReport {
    pub topology: String,
    /// Total actions in the run — the population.
    pub actions: usize,
    pub agents: usize,
    /// Distinct trees the topology assigned — the observable partition.
    pub trees: usize,
    /// Expected number of actions an observed action is indistinguishable
    /// from: `sum(n_i^2) / N`, the size-weighted mean of squared per-tree
    /// action counts. Equal to `actions` when one tree holds everything. When
    /// each agent has its own tree, this equals that agent's own action count
    /// only if every agent contributed the same number of actions; with
    /// unequal per-agent counts it is the size-weighted mean described above,
    /// not a simple per-agent figure.
    pub effective_anonymity_set: f64,
    /// Shannon entropy over the tree-size distribution, in bits. Zero when a
    /// single tree holds every action.
    pub entropy_bits: f64,
    /// Trees holding exactly one action. That action's anonymity set is
    /// itself.
    pub singleton_trees: usize,
    /// Smallest tree, by action count.
    pub min_tree: usize,
    /// Median tree, by action count.
    pub median_tree: usize,
    /// Largest tree, by action count.
    pub max_tree: usize,
}

/// Measure what a topology reveals, given how many actions each agent
/// performed.
///
/// The mapping onto `occultation`'s API, stated explicitly because it is a
/// different population than that crate was written for:
///
/// | occultation | here |
/// | --- | --- |
/// | a `Host` | an agent |
/// | `Host::count` | that agent's action count |
/// | the TCB fingerprint | the `TreeId` the topology assigned |
/// | a partition | the actions sharing one tree |
///
/// The question is identical in shape — a population, an observable
/// fingerprint, and how far the fingerprint narrows the population — which is
/// why the arithmetic transfers unchanged.
///
/// An agent that performed zero actions during the measured window
/// contributes nothing to the population and cannot belong to any anonymity
/// set — an idle agent is an absence, not a fault, and a real run can easily
/// contain one. Such agents are filtered out before the fleet is built rather
/// than rejected as an error, which is why `agents` below can be smaller than
/// `actions_per_agent.len()`: it counts only the agents that actually
/// contributed at least one action, not every agent named in the input.
pub fn measure(
    kind: TopologyKind,
    actions_per_agent: &[(String, usize)],
) -> Result<LinkabilityReport, LinkabilityError> {
    let contributing: Vec<(String, usize)> = actions_per_agent
        .iter()
        .filter(|(_, count)| *count > 0)
        .cloned()
        .collect();
    if contributing.is_empty() {
        return Err(LinkabilityError::NoActions);
    }

    let host = |(agent, count): &(String, usize)| {
        let mut tcb = BTreeMap::new();
        // The single "attribute" is the tree the topology assigned. This is
        // the fingerprint an observer of an inclusion proof learns.
        tcb.insert("tree".to_string(), kind.tree_for(agent).0);
        OccHost {
            id: agent.clone(),
            count: *count,
            tcb,
        }
    };

    // Every element of `contributing` has count > 0 (filtered above) and
    // `host` always inserts exactly one `tcb` entry, so `fleet.host` is
    // non-empty with no zero-count or empty-TCB member: none of
    // `occultation::anonymity::partition`'s error conditions can trigger
    // here. See `LinkabilityError::Partition`'s doc comment.
    let fleet = Fleet {
        name: kind.name().to_string(),
        host: contributing.iter().map(host).collect(),
    };
    let report = partition(&fleet)?;

    Ok(LinkabilityReport {
        topology: kind.name().to_string(),
        actions: report.fleet_size,
        agents: contributing.len(),
        trees: report.partitions.len(),
        effective_anonymity_set: report.effective_set,
        entropy_bits: report.entropy_bits,
        singleton_trees: report.singletons,
        min_tree: report.min_set,
        median_tree: report.median_set,
        max_tree: report.max_set,
    })
}
