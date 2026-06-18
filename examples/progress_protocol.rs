//! Phase 8.2 — Downstream adapter sufficiency check.
//!
//! Implements a minimal three-layer distributed progress protocol entirely on top of
//! the `antichain` core, with no network I/O or coordination. The goal is to confirm
//! that Phases 1–7 expose every primitive a real adapter needs; any genuine gap would
//! motivate graduating a deferred type (`SumOrder`, `IntervalSetLattice`) to implemented.
//!
//! # Protocol layers
//!
//! | Layer | Model | Type |
//! |-------|-------|------|
//! | Worker | Self-reported sequence number | `u64` |
//! | Shard | Aggregates a dynamic set of workers | `MapLattice<WorkerId, u64>` |
//! | Cluster | Merges all shard reports | `Frontier<u64>` |
//!
//! The `MapLattice` in the shard layer handles workers joining at runtime — new worker
//! keys appear in the map the moment they first report — without any schema change or
//! coordinator involvement. This validates the Phase 7.1 design goal.
//!
//! # Findings
//!
//! All three layers are expressible with the current API. No missing primitives were
//! found. `SumOrder` and `IntervalSetLattice` were not needed. The `WithBottom` sentinel
//! correctly models "shard not yet started" without magic constants.

use antichain::{Frontier, Lattice, MapLattice, WithBottom};

// ── Type aliases ──────────────────────────────────────────────────────────────

type WorkerId = u64;
type ShardId = u64;
type SeqNo = u64;

// ── Layer 1: Worker progress ──────────────────────────────────────────────────

/// A worker's progress is its current sequence number.
/// Workers advance monotonically; no coordinator is needed.
fn worker_advance(seq: SeqNo) -> SeqNo {
    seq
}

// ── Layer 2: Shard progress ───────────────────────────────────────────────────

/// A shard's state: a `MapLattice<WorkerId, SeqNo>` that grows as workers join.
///
/// `MapLattice::meet` (key-intersection + value-meet) gives the most conservative
/// progress across only the workers both observers have seen — ideal for the
/// "all-acknowledged" semantics. `join` (key-union + value-join) merges partial views.
fn shard_join(a: &MapLattice<WorkerId, SeqNo>, b: &MapLattice<WorkerId, SeqNo>)
    -> MapLattice<WorkerId, SeqNo>
{
    a.join(b)
}

/// Convert a shard's map into a `Frontier<u64>`: the antichain of the minimum
/// sequence number across all workers. For totally-ordered u64 this is width 1.
fn shard_frontier(shard: &MapLattice<WorkerId, SeqNo>) -> Frontier<SeqNo> {
    Frontier::from_elements(shard.values().copied())
}

// ── Layer 3: Cluster progress ─────────────────────────────────────────────────

/// Global progress: coordinator-free meet across all shard frontiers.
fn cluster_frontier(shards: &MapLattice<ShardId, SeqNo>) -> Frontier<SeqNo> {
    Frontier::from_elements(shards.values().copied())
}

// ── Scenario ──────────────────────────────────────────────────────────────────

fn main() {
    // ── Setup: two shards, each with two initial workers ─────────────────────

    let mut shard0: MapLattice<WorkerId, SeqNo> = MapLattice::new();
    shard0.insert(0, worker_advance(100));
    shard0.insert(1, worker_advance(80));

    let mut shard1: MapLattice<WorkerId, SeqNo> = MapLattice::new();
    shard1.insert(2, worker_advance(95));
    shard1.insert(3, worker_advance(110));

    // ── Layer 2: shard frontiers ──────────────────────────────────────────────

    let sf0 = shard_frontier(&shard0); // min(100, 80) = 80
    let sf1 = shard_frontier(&shard1); // min(95, 110) = 95

    assert_eq!(sf0.elements(), &[80u64], "shard 0 frontier");
    assert_eq!(sf1.elements(), &[95u64], "shard 1 frontier");
    println!("Shard 0 frontier: {:?}", sf0.elements());
    println!("Shard 1 frontier: {:?}", sf1.elements());

    // ── Layer 3: cluster frontier ─────────────────────────────────────────────

    // Build a flat MapLattice of all workers across both shards for cluster view.
    let cluster_map = shard_join(&shard0, &shard1);
    let cf = cluster_frontier(&cluster_map); // min(100, 80, 95, 110) = 80
    assert_eq!(cf.elements(), &[80u64], "cluster frontier");
    println!("Cluster frontier: {:?}", cf.elements());

    // Alternatively: meet of the two shard frontiers gives the same result.
    let cf_via_meet = sf0.meet(&sf1);
    assert_eq!(cf_via_meet, cf, "cluster via meet == cluster via map");

    // ── Dynamic worker join: no schema change required ────────────────────────

    shard0.insert(4, worker_advance(70)); // new worker joins mid-flight
    let sf0_updated = shard_frontier(&shard0); // min(100, 80, 70) = 70
    assert_eq!(sf0_updated.elements(), &[70u64], "shard 0 after new worker");
    println!("Shard 0 after worker 4 joins: {:?}", sf0_updated.elements());

    // ── Coordinator-free convergence guarantee ────────────────────────────────

    // Any ordering of updates produces the same result.
    let updates = [100u64, 80, 95, 110, 70];
    let observer_a = updates.iter().fold(Frontier::bottom(), |acc, &u| {
        acc.meet(&Frontier::from_elem(u))
    });
    let observer_b = updates.iter().rev().fold(Frontier::bottom(), |acc, &u| {
        acc.meet(&Frontier::from_elem(u))
    });
    assert_eq!(observer_a, observer_b, "convergence: order independent");
    println!("Convergence verified: both observers → {:?}", observer_a.elements());

    // ── WithBottom: 'shard not yet started' sentinel ──────────────────────────

    // A shard that has not reported yet is modeled with WithBottom::Bottom
    // instead of a magic constant like u64::MAX or 0.
    let not_started: Frontier<WithBottom<SeqNo>> =
        Frontier::from_elem(WithBottom::Bottom);
    let started: Frontier<WithBottom<SeqNo>> =
        Frontier::from_elem(WithBottom::Value(50));

    // Meet with a not-started shard is conservatively Bottom.
    let merged = not_started.meet(&started);
    assert_eq!(merged.elements(), &[WithBottom::Bottom], "not-started absorbs meet");
    println!("WithBottom sentinel: not_started.meet(started) = Bottom ✓");

    // ── MapLattice: partial-view merge ────────────────────────────────────────

    // Two observers see disjoint shards; their join produces the unified view.
    let mut view_a: MapLattice<ShardId, SeqNo> = MapLattice::new();
    view_a.insert(0, 80); // shard 0 min
    let mut view_b: MapLattice<ShardId, SeqNo> = MapLattice::new();
    view_b.insert(1, 95); // shard 1 min

    let unified = view_a.join(&view_b);
    let global = cluster_frontier(&unified); // min(80, 95) = 80
    assert_eq!(global.elements(), &[80u64], "unified cluster");
    println!("Unified cluster (partial views merged): {:?}", global.elements());

    // ── Summary ───────────────────────────────────────────────────────────────

    println!();
    println!("Phase 8.2 adapter sufficiency check: PASSED");
    println!("API exercised:");
    println!("  Frontier<u64>         — Phases 1, 2");
    println!("  MapLattice<K, u64>    — Phase 7.1");
    println!("  Frontier<WithBottom>  — Phase 7.3");
    println!("  Frontier::meet/join   — Phase 1");
    println!("No missing primitives found. SumOrder and IntervalSetLattice not needed.");
}
