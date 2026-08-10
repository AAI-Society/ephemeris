use ephemeris::topology::{AnchorPolicy, Global, Joint, PerAgent, Topology};

#[test]
fn global_puts_every_agent_in_one_tree() {
    let t = Global;
    assert_eq!(t.tree_for("agent-a"), t.tree_for("agent-b"));
    assert_eq!(t.anchor_policy(), AnchorPolicy::None);
}

#[test]
fn per_agent_gives_each_agent_its_own_tree() {
    let t = PerAgent;
    assert_ne!(t.tree_for("agent-a"), t.tree_for("agent-b"));
    assert_eq!(t.tree_for("agent-a"), t.tree_for("agent-a"));
}

#[test]
fn joint_partitions_per_agent_and_declares_an_interval() {
    let t = Joint { interval_secs: 60 };
    assert_ne!(t.tree_for("agent-a"), t.tree_for("agent-b"));
    assert_eq!(t.anchor_policy(), AnchorPolicy::Joint { interval_secs: 60 });
}

#[test]
fn the_tree_key_is_derived_from_the_whole_agent_id() {
    // The identity rule. Two agent ids differing anywhere must not collide,
    // and the same id must always map to the same tree.
    let t = PerAgent;
    assert_ne!(
        t.tree_for("did:web:example.org:agents:a"),
        t.tree_for("did:web:example.org:agents:b")
    );
    assert_ne!(t.tree_for("a"), t.tree_for("a "));
    assert_eq!(
        t.tree_for("did:web:example.org:agents:a"),
        t.tree_for("did:web:example.org:agents:a")
    );
}

#[test]
fn a_global_tree_id_does_not_collide_with_any_agent_id() {
    // Global's key must be unambiguous, not merely constant: an agent
    // literally named "global" must not land in the same namespace.
    assert_ne!(Global.tree_for("global"), PerAgent.tree_for("global"));
    assert_ne!(Global.tree_for("x"), PerAgent.tree_for("global"));
}

#[test]
fn a_forest_routes_leaves_by_topology() {
    use ephemeris::topology::Forest;
    let mut g = Forest::new(Global);
    let mut p = Forest::new(PerAgent);
    for (agent, leaf) in [("a", [1u8; 32]), ("b", [2u8; 32]), ("a", [3u8; 32])] {
        g.insert(agent, leaf);
        p.insert(agent, leaf);
    }
    assert_eq!(g.tree_count(), 1);
    assert_eq!(p.tree_count(), 2);
}

#[test]
fn the_same_leaves_produce_the_same_records_under_every_topology() {
    // The design's equivalence property. A topology decides *where* a leaf
    // lands, never *what* it says. If a topology could change a record, the
    // three would not be comparable and the Phase 2 benchmark would be
    // measuring three different systems rather than three placements of one.
    use ephemeris::topology::Forest;
    let leaves = [("a", [1u8; 32]), ("b", [2u8; 32]), ("a", [3u8; 32])];

    let mut g = Forest::new(Global);
    let mut p = Forest::new(PerAgent);
    let mut j = Forest::new(Joint { interval_secs: 60 });
    for (agent, leaf) in leaves {
        g.insert(agent, leaf);
        p.insert(agent, leaf);
        j.insert(agent, leaf);
    }

    // Every topology saw every leaf, exactly once.
    assert_eq!(g.leaf_count(), 3);
    assert_eq!(p.leaf_count(), 3);
    assert_eq!(j.leaf_count(), 3);

    // PerAgent and Joint partition identically; only their anchor policy
    // differs. If this ever fails, Joint has become a third partitioning
    // rather than a per-agent one with anchoring.
    assert_eq!(p.roots(), j.roots());
    assert_ne!(g.roots(), p.roots());
}
