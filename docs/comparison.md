# Prior art & positioning

How `antichain` relates to other crates and designs that address overlapping problems.

---

## timely-dataflow / differential-dataflow

[timely-dataflow](https://github.com/TimelyDataflow/timely-dataflow) and
[differential-dataflow](https://github.com/TimelyDataflow/differential-dataflow) are the
direct intellectual ancestors of this crate. The `Antichain` and `Frontier` types here
implement the same algebraic objects described in Frank McSherry's work on Naiad-style
distributed dataflow.

### What they share

- The same mathematical foundation: antichain-based progress tracking, lattice `meet`
  as the coordinator-free merge, and the convergence guarantee that follows.
- The same `less_equal` / `less_than` interface for testing whether a timestamp is
  past the frontier.
- Property-tested algebraic laws (commutativity, associativity, idempotence).

### Where `antichain` differs

| Dimension | timely/differential | `antichain` |
|-----------|--------------------|----|
| **Dependency footprint** | Full dataflow runtime (scheduling, communication, workers) | Zero runtime dependencies; pure data type |
| **`no_std`** | Requires `std` | `no_std` + `alloc` with feature flag |
| **Composition toolkit** | Fixed set of timestamp types | Composition-first: `ProductTimestamp`, `Lexicographic`, `Max`, `Min`, `Bounded`, `WithTop`, `WithBottom`, `MapLattice`, `SetLattice`, `IntervalSetLattice` |
| **Formal verification** | Informally argued | Fizzbee model-checked convergence spec |
| **Scope** | Full distributed dataflow engine | Progress tracking only — no scheduler, no channels |

**When to use timely/differential instead:** you are building a full dataflow computation
graph and want the scheduling, communication, and worker management included. `antichain`
does not provide those; it only provides the progress primitive you would build on top of.

**When to use `antichain`:** you need the progress-tracking primitive in isolation — as
part of a custom runtime, embedded in a `no_std` context, or as a dependency-light
component of a larger system that rolls its own transport.

---

## CRDTs (conflict-free replicated data types)

[crdts](https://crates.io/crates/crdts), [rust-crdt](https://crates.io/crates/rust-crdt),
and similar libraries implement conflict-free replicated *data* structures — counters,
sets, maps, sequences — designed to replicate state between nodes without coordination.

### What they share

The algebraic foundation is identical. A CRDT's merge operation is a semilattice join
(commutative, associative, idempotent), and `Frontier::meet` satisfies exactly the same
laws. Both exploit the same mathematical insight: if your merge function is a lattice
operation, you can eliminate the coordinator.

### Where they differ

| Dimension | CRDTs | `antichain` |
|-----------|-------|-------------|
| **What is being tracked** | Replicated application *data* (sets, counters, maps) | *Progress* — how far computation has advanced |
| **Merge direction** | Usually `join` (least upper bound — grow toward `⊤`) | Typically `meet` (greatest lower bound — conservative bound toward `⊥`) |
| **Semantics** | "What data is present" | "What timestamps are past the frontier" |
| **Causality / version vectors** | Often first-class (Dot stores, vector clocks) | Not included — separate concern |

**When to use a CRDT library instead:** you want replicated application-level state
(a shared counter, a collaborative set, a LWW register). CRDTs model the *data* your
application stores; `antichain` models the *time* at which processing has arrived.

**When to combine them:** common pattern — use a CRDT library for the application data
and `antichain` for the progress fence that guards when that data is safe to read.

---

## `min_max_heap`, priority queues, and ordered sets

`Antichain<T>` might look superficially like a priority queue or a sorted set, but it
is neither:

- A **priority queue** maintains total order and surfaces the minimum or maximum. An
  antichain maintains *mutual incomparability* — all elements are kept precisely because
  none of them dominate the others. There is no single minimum.
- A **sorted set** (`BTreeSet`) orders elements and can answer range queries. An
  antichain keeps only the *Pareto frontier* of a partial order — elements that are not
  dominated by any other. The invariant-maintaining `insert` is the key difference.

---

## Summary

`antichain` is the right tool when:

1. You need coordinator-free progress tracking (not general CRDT data replication).
2. You want a standalone, dependency-light crate (`no_std`, zero runtime dependencies).
3. Your time domain is multi-dimensional and you want a composition toolkit.
4. You want a formally verified convergence guarantee.

Use timely-dataflow if you want the full dataflow engine. Use a CRDT library if you are
replicating application data rather than tracking computation progress.
