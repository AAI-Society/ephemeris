use crate::bench::TopologyKind;
use occultation::anonymity::{partition, AnonymityError, Fleet, Host as OccHost};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum LinkabilityError {
    #[error("a run with no actions has no anonymity to report")]
    NoActions,
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
    /// from: `sum(n_i^2) / N`. Equal to `actions` when one tree holds
    /// everything, and to the per-agent action count when each agent has its
    /// own tree.
    pub effective_anonymity_set: f64,
    /// Shannon entropy over the tree-size distribution, in bits. Zero when a
    /// single tree holds every action.
    pub entropy_bits: f64,
    /// Trees holding exactly one action. That action's anonymity set is
    /// itself.
    pub singleton_trees: usize,
    pub min_tree: usize,
    pub median_tree: usize,
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
pub fn measure(
    kind: TopologyKind,
    actions_per_agent: &[(String, usize)],
) -> Result<LinkabilityReport, LinkabilityError> {
    let total: usize = actions_per_agent.iter().map(|(_, n)| *n).sum();
    if actions_per_agent.is_empty() || total == 0 {
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

    let fleet = Fleet {
        name: kind.name().to_string(),
        host: actions_per_agent.iter().map(host).collect(),
    };
    let report = partition(&fleet)?;

    Ok(LinkabilityReport {
        topology: kind.name().to_string(),
        actions: report.fleet_size,
        agents: actions_per_agent.len(),
        trees: report.partitions.len(),
        effective_anonymity_set: report.effective_set,
        entropy_bits: report.entropy_bits,
        singleton_trees: report.singletons,
        min_tree: report.min_set,
        median_tree: report.median_set,
        max_tree: report.max_set,
    })
}
