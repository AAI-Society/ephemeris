#![cfg(feature = "bench")]

use ephemeris::bench::linkability::measure;
use ephemeris::bench::TopologyKind;

fn fleet(agents: usize, each: usize) -> Vec<(String, usize)> {
    (0..agents).map(|a| (format!("agent-{a}"), each)).collect()
}

#[test]
fn a_global_tree_makes_every_action_indistinguishable() {
    // The P05 answer. One tree, so membership distinguishes nobody and the
    // effective anonymity set is the whole population.
    let r = measure(TopologyKind::Global, &fleet(10, 20)).unwrap();
    assert_eq!(r.trees, 1);
    assert_eq!(r.actions, 200);
    assert!((r.effective_anonymity_set - 200.0).abs() < 1e-9);
    assert!(r.entropy_bits.abs() < 1e-9, "one tree cannot carry entropy");
    assert_eq!(r.singleton_trees, 0);
}

#[test]
fn a_per_agent_tree_narrows_an_action_to_its_agent() {
    // The same run, the other topology. Each tree holds exactly one agent's
    // actions, so observing a tree names the agent.
    let r = measure(TopologyKind::PerAgent, &fleet(10, 20)).unwrap();
    assert_eq!(r.trees, 10);
    assert_eq!(r.actions, 200);
    assert!((r.effective_anonymity_set - 20.0).abs() < 1e-9);
    assert!(r.entropy_bits > 3.32, "10 equal trees carry log2(10) bits");
}

#[test]
fn joint_partitions_exactly_as_per_agent_does() {
    // Phase 1's topology test already pins that Joint and PerAgent partition
    // identically; anchoring changes ordering, not partitioning. If this ever
    // fails, Joint has become a third partitioning and that is a bug rather
    // than a finding.
    let f = fleet(7, 13);
    let p = measure(TopologyKind::PerAgent, &f).unwrap();
    let j = measure(TopologyKind::Joint, &f).unwrap();
    assert_eq!(p.trees, j.trees);
    assert_eq!(p.singleton_trees, j.singleton_trees);
    assert!((p.effective_anonymity_set - j.effective_anonymity_set).abs() < 1e-9);
    assert!((p.entropy_bits - j.entropy_bits).abs() < 1e-9);
}

#[test]
fn a_lone_agent_is_a_singleton_under_per_agent_and_not_under_global() {
    // The asymmetry stated as a test: the same fleet, two topologies, and a
    // one-action agent is either hidden or exposed depending only on the
    // topology.
    let f = vec![("busy".to_string(), 100), ("lonely".to_string(), 1)];
    let g = measure(TopologyKind::Global, &f).unwrap();
    let p = measure(TopologyKind::PerAgent, &f).unwrap();
    assert_eq!(g.singleton_trees, 0);
    assert_eq!(p.singleton_trees, 1);
    assert!(p.min_tree == 1 && g.min_tree == 101);
}

#[test]
fn an_empty_fleet_is_refused_rather_than_reported_as_perfect_anonymity() {
    // Zero actions must not report an effective set of zero and read as a
    // measurement. It is an absence.
    assert!(measure(TopologyKind::Global, &[]).is_err());
}

#[test]
fn an_idle_agent_is_filtered_rather_than_treated_as_an_error() {
    // A mixed fleet: some agents acted, one did not. An agent with zero
    // actions during the measured window is normal, not a fault, so it must
    // not turn the whole measurement into an error, and it must not be
    // counted among the agents that contributed.
    let f = vec![
        ("busy".to_string(), 50),
        ("idle".to_string(), 0),
        ("also-busy".to_string(), 50),
    ];
    let r = measure(TopologyKind::Global, &f).unwrap();
    assert_eq!(r.actions, 100, "the idle agent's zero actions add nothing");
    assert_eq!(
        r.agents, 2,
        "the idle agent must not be counted among contributing agents"
    );
}

#[test]
fn no_error_message_measure_can_produce_leaks_occultation_vocabulary() {
    // occultation::anonymity's errors speak of fleets, hosts and TCB
    // attributes -- exactly the vocabulary this module exists to keep out of
    // anything ephemeris-facing, error text included. Exercise every path
    // `measure` can actually take to an `Err` and check the rendered message
    // against occultation's words.
    let forbidden = ["host", "fleet", "tcb"];
    let messages = [
        // Fully empty input.
        measure(TopologyKind::Global, &[]).unwrap_err().to_string(),
        // Every named agent is idle -- the all-zero case, distinct from the
        // fully-empty one but the other input that must still be refused.
        measure(TopologyKind::Global, &[("idle".to_string(), 0)])
            .unwrap_err()
            .to_string(),
    ];
    for msg in messages {
        let lower = msg.to_lowercase();
        for word in forbidden {
            assert!(
                !lower.contains(word),
                "error message `{msg}` must not contain occultation vocabulary `{word}`"
            );
        }
    }
}
