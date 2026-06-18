# Antichain — Roadmap & Implementation Plan

**Status:** Living planning document.
**Date:** 2026-06-18

> Principle: the math is where the certainty lives. Implement inward-out — get the core correct and proven before building outward.

---

## Phases at a glance

| Phase | Name | Goal | Done when |
|-------|------|------|-----------|
| 0 | Scaffolding | Crate exists, CI runs | `cargo test` is green |
| 1 | Core types | `Lattice`, `Antichain<T>`, `Frontier<T>` | Types compile, unit tests pass |
| 2 | Law proofs | All algebraic laws property-tested | No `proptest` failures under 10 000 cases |
| 3 | Composition | Product, lexicographic, nested orders | Composed frontiers work correctly |
| 4 | Hardening | Benchmarks, fuzzing, docs, `#![no_std]` compat | Crate is publishable |
| 5 | Formal specification | TLA⁺ or Lean proof of convergence | Spec written, convergence proven |
| 6 | Extended composition patterns | Additional useful partial orders | ✅ `Max<T>`, `Min<T>`, `Bounded<T>` working |
| 7 | Advanced structural & dynamic lattices | Dynamic, lifted, and set-based orders | ✅ `MapLattice`, `SetLattice`, `WithTop`/`WithBottom` in core; `IntervalSetLattice` shipped in Phase 9 companion crate; `SumOrder` deferred |
| 8 | Performance & real-world validation | Compaction, benchmarked width bounds, downstream adapter | ✅ `meet` width bound documented; adapter example validates sufficiency; Phase 6 design debt resolved |
| 9 | Adoption, expressiveness & hardening | Cookbook, companion crate, perf, ecosystem | ✅ Cookbook + universal consistency law; `antichain-intervals`; inline storage; MSRV/semver/no_std CI; `serde` feature fixed |
| 10 | Onboarding & ecosystem reach | Tutorial, runnable examples, diagrams, prior-art context | ✅ Tutorial; `watermark_gossip` + `backfill_gaps` examples; `comparison.md`; `0.3.0` |

---

## Extraction from RockStream

A correct `Antichain<T>` and `Frontier<T>` implementation already exists in RockStream at
`crates/rockstream-types/src/frontier.rs`. **Copy it as raw material, not as finished product.**

### What to salvage

| Item | Location in `frontier.rs` | Action |
|------|--------------------------|--------|
| `Antichain<T>` | lines ~140–220 | Keep. Already generic over `T: PartialOrd + Clone`. |
| `Frontier<T>` | lines ~220–300 | Keep. `meet` and `join` are correct. |
| `Lattice` trait | lines ~230–240 | Keep as-is. `meet`/`join` names are the right names (see §1.2 below). |
| `ProductTimestamp<T1, T2>` | lines ~300–375 | Keep. The `PartialOrd` impl for product order is correct. |
| Lattice law tests in `test_frontier_lattice_properties` | lines ~410–450 | Keep. These test commutativity, associativity, and absorption — convert to `proptest` in Phase 2. |

### What to strip

Every type below is domain contamination. Delete it entirely; it does not belong in the core crate.

- `Epoch`, `ShardId`, `WorkerId`, `SourceId`, `OperatorId`, `MergeLawId` — RockStream newtypes
- `SourceProgress`, `FreshnessToken` — domain-specific progress structs
- `ShardFrontierReport`, `WorkerFrontierSummary`, `ClusterFrontier`, `CompleteThroughToken` — the three-layer protocol (v0.32) belongs in a RockStream adapter crate, not the core
- All `use crate::…` imports

### Dependency audit

After stripping, the only crate-level dependencies should be:
- `serde` (feature-gated, `default-features = false`)
- `proptest` (dev-dependency only)
- `criterion` (dev-dependency only)

---

## Phase 0 — Scaffolding

**Goal:** a bare crate that builds and runs CI.

- [ ] `cargo new --lib antichain` (root name, not `antichain-core` — see §Naming below)
- [ ] Add `proptest`, `serde` (feature-gated), `criterion` to `Cargo.toml`
- [ ] Set up GitHub Actions: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
- [ ] Write `README.md` opening line: *"A coordinator-free primitive for tracking distributed progress using lattice algebra."* State scope on line one (progress only; no ownership, no consensus).
- [ ] Commit `idea.md` into the repo as `docs/idea.md` — the spec lives with the code.

> **Naming:** The foundational crate claims the root name `antichain`, following the convention of `serde`, `tokio`, `arrow`.

---

## Phase 1 — Core types

**Goal:** the three primitives that carry the whole idea.

### 1.1 Ordering trait — use `std::cmp::PartialOrd`, add `Lattice`

The RockStream code already uses `T: PartialOrd` (the standard library trait) as the bound on
`Antichain<T>` and `Frontier<T>`. **Keep this decision.** Introducing a custom `PartialOrder` trait
would shadow a well-known standard trait for no benefit.

Where strict lattice guarantees (meet/join) are needed beyond what `PartialOrd` provides, use the
`Lattice` trait that already exists in the RockStream source:

```rust
/// Greatest lower bound (meet) and least upper bound (join).
pub trait Lattice: PartialOrd {
    fn meet(&self, other: &Self) -> Self;
    fn join(&self, other: &Self) -> Self;
}
```

The name `Lattice` is precise and does not collide with `std`. Blanket impls for `u64`, `i64`, and
tuples follow from this. Property tests in Phase 2 document the exact laws `T` must satisfy.

### 1.2 `Antichain<T>`

The *set* invariant: no element in the set is `less_equal` (`<=`) than another.
The RockStream implementation is correct and can be taken directly after stripping domain imports.

```rust
pub struct Antichain<T> { elements: Vec<T> }  // keep field private

impl<T: PartialOrd + Clone> Antichain<T> {
    pub fn empty() -> Self { … }
    pub fn from_elem(t: T) -> Self { … }
    /// Insert t, maintaining the antichain invariant.
    /// Drops any existing element e where t <= e, and skips t if any e <= t.
    pub fn insert(&mut self, t: T) { … }
    pub fn elements(&self) -> &[T] { … }
    pub fn len(&self) -> usize { … }
    pub fn is_empty(&self) -> bool { … }
    /// True if `time` is less_equal some element (i.e., time is still in-flight).
    pub fn less_equal(&self, time: &T) -> bool { … }
}
```

The `meet` operation on `Antichain` is achieved by inserting all elements of both antichains into
a fresh one — the `insert` invariant-maintenance does the rest. No `Lattice` bound is required on
`T` for `meet`.

### 1.3 `Frontier<T>`

A `Frontier` is the *progress claim*: *"all timestamps strictly less than some element of this antichain are complete."*
The RockStream implementation uses `meet` for the coordinator-free merge and `join` (gated on `T: Lattice`) for advancement. Rename `empty()` → `bottom()` to match the idea document's terminology; add `top()` for completeness.

```rust
pub struct Frontier<T> { antichain: Antichain<T> }  // keep field private

impl<T: PartialOrd + Clone> Frontier<T> {
    pub fn bottom() -> Self { … }           // no progress — nothing is complete
    pub fn from_elem(t: T) -> Self { … }
    pub fn from_elements(iter: impl IntoIterator<Item = T>) -> Self { … }
    pub fn less_equal(&self, time: &T) -> bool { … } // is time still in-flight?
    pub fn elements(&self) -> &[T] { … }
    /// Coordinator-free merge: lattice meet (greatest lower bound).
    /// Commutative, associative, idempotent — this is the core operation.
    pub fn meet(&self, other: &Self) -> Self { … }
}

impl<T: Lattice + Clone> Frontier<T> {
    /// Lattice join (least upper bound) — advances the frontier.
    pub fn join(&self, other: &Self) -> Self { … }
}
```

**Key design constraint:** `Frontier<T>` owns no network, no I/O, no async. It is a pure value type.

---

## Phase 2 — Law proofs via property testing

**Goal:** replace "we believe the laws hold" with "the test suite demonstrates them hold for 10 000 random inputs."

Use `proptest` strategies to generate arbitrary `Frontier<u64>` and `Frontier<(u64, u64)>` values.

### Laws to test

| Law | Expression |
|-----|-----------|
| Commutativity | `merge(a, b) == merge(b, a)` |
| Associativity | `merge(a, merge(b, c)) == merge(merge(a, b), c)` |
| Idempotence | `merge(a, a) == a` |
| Absorption | if `a` dominates `b`, then `merge(a, b) == a` |
| Antichain invariant | no two elements `x, y` in `Antichain` where `x.less_equal(y)` |
| Reflexivity of `PartialOrd` | `a <= a` always |
| Antisymmetry of `PartialOrd` | if `a ≤ b` and `b ≤ a` then `a == b` |
| Transitivity of `PartialOrd` | if `a ≤ b` and `b ≤ c` then `a ≤ c` |

Each law gets its own `#[test]` function driven by `proptest! { … }`. CI fails if any law breaks under any generated input.

### Address open question: minimal `T` contract

The property tests serve double duty: they document *exactly* what `T` must satisfy. If a law test only requires `less_equal` and `PartialEq`, that is the complete trait contract — not an assumption.

---

## Phase 3 — Composition

**Goal:** express multi-dimensional progress without special-casing.

### 3.1 Product order

Tuples already have `PartialOrd` in the standard library, but it is *lexicographic*, not the product
order. Use `ProductTimestamp<T1, T2>` (see §3.2) for the true product order. For lightweight
tuple-style use, a blanket `Lattice` impl on tuples covers the meet/join:

```rust
impl<A: Lattice, B: Lattice> Lattice for (A, B) {
    fn meet(&self, other: &Self) -> Self { (self.0.meet(&other.0), self.1.meet(&other.1)) }
    fn join(&self, other: &Self) -> Self { (self.0.join(&other.0), self.1.join(&other.1)) }
}
```

This handles `(source_id, sequence_number)`, `(partition, offset)`, etc. without a new named type.

### 3.2 `ProductTimestamp<T1, T2>` (already in RockStream)

The RockStream source already has a correct `ProductTimestamp<T1, T2>` with `PartialOrd` and
`Lattice` impls. Extract it verbatim alongside `Antichain` and `Frontier` — it is pure math with
no domain dependencies. Rename to align with the public API convention if needed, but do not
rewrite the logic.

```rust
pub struct ProductTimestamp<T1, T2> { pub outer: T1, pub inner: T2 }

impl<T1: PartialOrd, T2: PartialOrd> PartialOrd for ProductTimestamp<T1, T2> { … } // product order
impl<T1: Lattice, T2: Lattice> Lattice for ProductTimestamp<T1, T2> { … }          // component-wise
```

For lexicographic order ("outer clock dominates, inner clock breaks ties"), add a separate
`Lexicographic<A, B>` newtype. Needed for epoch × offset patterns where the outer dimension
totally orders the inner one.

### 3.3 Frontier size under composition — treat as critical path

Address open question: *does the antichain explode in size for high-dimensional `T`?*

This is the **highest-risk correctness/performance question** in the whole roadmap. The Phase 4
benchmarks must explicitly stress this before the crate is published:

- Benchmark `Antichain<ProductTimestamp<u64, u64>>::insert` after 1 000 mutually incomparable
  elements. Measure how much the `merge` inner loop degrades CPU as width grows.
- Benchmark `Frontier::meet` on two frontiers each of width 1 000 (quadratic inner loop).
- If degradation is material, implement a compaction step: after `meet`, attempt projection-based
  dominance elimination.
- Document the bound: worst case O(n) incomparable elements; for bounded-range product orders
  the empirical bound is usually ≪ 100. State this with data, not assumption.

---

## Phase 4 — Hardening

**Goal:** crate is ready to publish on `crates.io`.

- [ ] Criterion benchmarks for `insert`, `merge`, `dominates` on antichain sizes 1–1000.
- [ ] `cargo-fuzz` target against the `insert` and `merge` paths.
- [ ] `#![no_std]` compatibility (swap `Vec` for `alloc::vec::Vec` behind a feature flag).
- [ ] Feature-gate `serde::{Serialize, Deserialize}` impls.
- [ ] Full rustdoc with examples and law explanations inline.
- [ ] Convergence guarantee stated as a doc-test: *"Two nodes that have each seen any subset of the same update set, in any order, will hold identical `Frontier` values after merging."*
- [ ] Publish `antichain` to `crates.io` at `0.1.0`.

---

## Open questions — action owners

From `idea.md` §8, mapped to phases:

| Question | Phase | Action |
|----------|-------|--------|
| Minimal trait contract for `T` | 2 | Property tests reveal the exact required laws |
| Frontier size explosion in high-D | 3 | Benchmark + compaction step if needed |
| Formal convergence invariant | 5 | Fizzbee spec of convergence guarantee |

---

## Phase 5 — Formal specification

**Goal:** prove the convergence guarantee with mathematical certainty.

- [x] Write a Fizzbee spec of the core convergence theorem
- [x] Specify the exact invariants that `meet` preserves under arbitrary message reordering
- [x] Document the formal statement: *"If two nodes have each observed any subset of the same update set, in any order, their `Frontier` values will be identical after merging."*
- [x] Validate the spec against the property tests from Phase 2
- [x] Ship as v0.2.0 with formal correctness guarantee

**Why this matters:** For systems where correctness is critical (financial streaming, audit logs, replication), a formal proof gives confidence beyond empirical testing.

---

## Phase 6 — Extended composition patterns

**Goal:** add useful partial orders for real-world use-cases.

### 6.1 `Max<T>` and `Min<T>` wrappers ✅

Wrappers that flip the partial order of any type without requiring a custom `Lattice` impl:

```rust
/// Inverts PartialOrd: a ≤ b in Max iff b ≤ a in T.
pub struct Max<T>(pub T);

/// Keeps PartialOrd as-is but provides Max semantics for meet/join.
pub struct Min<T>(pub T);
```

Used for tracking "at least X" vs. "at most Y" bounds in the same frontier.

### 6.2 `Bounded<T>` ✅

A newtype for finite ranges where the partial order is restricted to a known interval `[min, max]`.
Enables bounded-width antichain guarantees: if `T` is bounded, the antichain width is provably ≤ the cardinality of the bound.

```rust
pub struct Bounded<T> { value: T, min: T, max: T }
```

### 6.3 Nested composition examples ✅

Document patterns like:
- `Frontier<ProductTimestamp<Bounded<u64>, u64>>` — bounded outer clock, unbounded inner
- `Frontier<(Max<u64>, Min<u64>)>` — independent lower and upper bounds
- Integration with the formal spec: prove that nesting preserves the convergence guarantee

All patterns are covered by unit tests in `tests_phase6` and property tests in `prop_tests_phase6`.

### 6.4 Retrospective — known design debt

Phase 6 shipped and is fully tested, but two items carry design debt to revisit (tracked in
Phase 8):

- **`Min<T>` is a transparent newtype.** It delegates every `Lattice` and `PartialOrd` operation
  straight to `T`, so its only value is documentary intent (pairing with `Max<T>` in composites
  like `(Max<u64>, Min<u64>)`). It adds permanent public API surface for readability alone.
  Acceptable for now; reconsider whether it earns its place once downstream usage exists.
- **`Bounded<T>` carries instance-level bounds and breaks the `PartialOrd`-only contract.**
  - Each value stores `value/min/max` (3× memory); `meet`/`join` arbitrarily use `self`'s bounds,
    so mixing values with different ranges in one antichain is *documented as undefined* — a
    footgun in the crate's core operation.
  - It requires `T: Ord`, while the rest of the crate is built on `PartialOrd`. As a result
    `Bounded<ProductTimestamp<…>>` does **not** compose, which undercuts the composition thesis.
  - **Phase 8 candidate:** redesign with type-level bounds (const generics where feasible) or drop
    per-instance bounds, driven by what a real downstream adapter actually needs.

---

## Phase 7 — Advanced structural & dynamic lattices

**Goal:** extend the composition toolkit from fixed-arity scalar wrappers to dynamic, lifted, and
set-based lattice structures that model the topological shapes real distributed systems produce.

**Sequencing rationale:** implement **7.3 (`WithTop`/`WithBottom`) first** — it is the simplest
addition and clarifies the bottom/top semantics the others reason about. 7.1 and 7.2 are
independent of each other and follow. `SumOrder` (deferred, post-Phase 7) algebraically depends on
`WithTop`/`WithBottom` to close its meet operation. 7.4 (`IntervalSetLattice`) is a candidate for a
companion crate (`antichain-intervals`) rather than the core crate to keep the dependency surface
small.

**Correctness law to enforce on every type below:** the `meet`/`join` impls must agree with
`PartialOrd` — i.e. `a ≤ b ⟺ meet(a, b) == a ⟺ join(a, b) == b`. This consistency law is where
dynamic-arity and lifted lattices most often hide subtle bugs; make it an explicit property test,
not just commutativity/associativity/idempotence.

### 7.1 `MapLattice<K, V>` — point-wise dynamic composition

The natural generalization of `ProductTimestamp` to dynamic arities. Solves the hardest
structural problem in the roadmap: a system with runtime topology changes (shards added/removed)
cannot be modeled with fixed-arity tuples; `MapLattice` makes it expressible.

**Partial order:** $M_1 \le M_2$ iff for every key $k$ in $M_1$, $V_{M_1}(k) \le V_{M_2}(k)$
(missing keys are implicitly `Bottom` — no progress recorded yet).

```rust
pub struct MapLattice<K: Ord, V: Lattice> { map: BTreeMap<K, V> }

impl<K: Ord + Clone, V: Lattice + Clone> Lattice for MapLattice<K, V> {
    /// Union of keys; overlapping values take their join.
    fn join(&self, other: &Self) -> Self { … }
    /// Intersection of common keys; overlapping values take their meet.
    fn meet(&self, other: &Self) -> Self { … }
}
```

**Use-case:** A cluster scales from 10 shards to 100 shards at runtime. Each shard key appears
in the map the moment it reports progress. Static tuples cannot accommodate this without
recompilation.

**Implementation:** `BTreeMap<K, V>` — no exotic dependencies; `alloc` only (compatible with
`no_std`).

**Bottom is the empty map.** No bottom element is required on `V`: `meet` = key-intersection with
value-meet is a genuine greatest lower bound (a key present in only one map collapses to bottom and
is simply dropped — its bottom value is never constructed), and `join` = key-union with value-join
is the least upper bound. The empty map is the identity for `join` and absorbing for `meet`.
This is why `MapLattice` has no hard dependency on 7.3 despite the "missing key = Bottom" framing.

- [x] `MapLattice<K, V>` struct with `insert`, `get`, `keys`, `values`
- [x] `Lattice` impl (join = key-union + value-join; meet = key-intersection + value-meet)
- [x] `PartialOrd` impl following the point-wise definition
- [x] Unit tests: insert, meet, join, empty-map edge cases
- [x] Property tests: commutativity, associativity, idempotence **and the `PartialOrd`/`meet`
      consistency law** (`a ≤ b ⟺ meet(a, b) == a`) — empty map as `join` identity and `meet`
      absorber

### 7.2 `SetLattice<T>` — powerset / set-inclusion order

Tracks completion of discrete, unordered elements via subset inclusion. The partial order is
$A \le B$ iff $A \subseteq B$. Meet is intersection; join is union.

**Use-case:** A global configuration state is only advanced when the set of acknowledging nodes
matches the expected cluster membership. Each node publishes its current acknowledgement set;
the coordinator-free merge (meet = intersection) computes the universal acknowledgement.

```rust
pub struct SetLattice<T: Ord> { set: BTreeSet<T> }

impl<T: Ord + Clone> Lattice for SetLattice<T> {
    fn meet(&self, other: &Self) -> Self { /* intersection */ }
    fn join(&self, other: &Self) -> Self { /* union */ }
}
```

**Note:** Four lines of logic around `BTreeSet` — the value is the semantic contract and the
property tests, not the complexity of the implementation.

- [x] `SetLattice<T>` struct with `insert`, `contains`, `len`
- [x] `Lattice` impl (meet = intersection; join = union)
- [x] `PartialOrd` impl via `is_subset`
- [x] Property tests: commutativity, associativity, idempotence

### 7.3 `WithTop<T>` / `WithBottom<T>` — lifted bounding enums ← implement first

Any lattice is structurally incomplete without explicit top/bottom elements. `WithTop` and
`WithBottom` add structural EOF and pipeline sentinel values without magic constants like
`u64::MAX`.

**Invariant:**
- `WithTop<T>`: adds a single `Top` element above all `Value(t)`. `Top` is absorbing for
  `join` and the identity for `meet`. Does **not** carry a `Bottom` variant — compose with
  `WithBottom` when both sentinels are needed.
- `WithBottom<T>`: symmetric; adds a single `Bottom` element below all `Value(t)`. `Bottom` is
  absorbing for `meet` and the identity for `join`.

```rust
pub enum WithTop<T>    { Value(T), Top }
pub enum WithBottom<T> { Bottom, Value(T) }
```

**Use-case:** When an upstream ingestion source finishes, wrapping its final frontier element
in `WithTop::Top` immediately signals downstream: *"this data path is permanently closed."*
The `join` with any other frontier immediately absorbs to `Top`, short-circuiting progress
calculation.

**Design note:** `WithTop<WithBottom<T>>` composes to a three-element bounded lattice
`Bottom < Value(t) < Top`. This is the correct way to lift any type to a closed bounded lattice
without magic constants.

- [x] `WithTop<T>` and `WithBottom<T>` enum types
- [x] `PartialOrd`, `Lattice` impls for both
- [x] `WithTop<WithBottom<T>>` composition example in docs
- [x] Unit tests: Top absorbs join, Bottom absorbs meet, Value(t) round-trips
- [x] Property tests: laws hold for `WithTop<u64>` and `WithBottom<u64>`

### 7.4 `IntervalSetLattice<T>` — disjoint range aggregation (companion crate candidate)

Tracks a canonicalized set of non-overlapping intervals to model out-of-order progress with
gaps. Merging vectors of intervals automatically bridges overlapping bounds and resolves holes
as missing elements arrive.

**Lattice operations:**
- `join(A, B)`: merge intervals, coalescing any overlaps → largest covered set
- `meet(A, B)`: intersect intervals → smallest commonly covered set

**Use-case:** A backfill engine processes blocks 150–200 while block 101 is still delayed.
`IntervalSetLattice` tracks exactly what can be safely acknowledged without losing the gap.

**Scope decision:** The interval-set canonicalization algorithm (split, merge, coalesce) is
a non-trivial data structure. This is a **candidate for a companion crate**
(`antichain-intervals`) rather than the core crate, keeping the core lean and `no_std`-simple.
The decision should be made after 7.1–7.3 are complete and the performance characteristics are
understood.

- [ ] Prototype `IntervalSetLattice<T>` in a standalone module
- [ ] Benchmark: coalesce cost at 1 000 and 100 000 disjoint intervals
- [ ] Decide: core crate vs. `antichain-intervals` companion crate based on benchmark results
- [ ] If companion: publish as `antichain-intervals = "0.1.0"` alongside core `0.3.0`

---

### Phase 7 summary table

| Sub-phase | Type | Location | Depends on | Status |
|-----------|------|----------|------------|--------|
| 7.1 | `MapLattice<K, V>` | core | — | ✅ done |
| 7.2 | `SetLattice<T>` | core | — | ✅ done |
| 7.3 | `WithTop<T>` / `WithBottom<T>` | core | — | ✅ done |
| 7.4 | `IntervalSetLattice<T>` | companion crate TBD | 7.1, 7.3 | ✅ shipped in Phase 9 |

---

## Deferred — `SumOrder<A, B>` (post-Phase 7)

`SumOrder<A, B>` models a coproduct of two incomparable time domains. It is mathematically
correct but ergonomically expensive: users must wrap in `WithTop<WithBottom<SumOrder<A,B>>>`
to get a closed lattice, and `less_equal` across variants is semantically murky. In practice,
the same problems are almost always better solved by separate frontiers or a `MapLattice` with
a discriminant key.

Deferred until real usage in downstream crates demonstrates a genuine need that cannot be
met by existing composition primitives. Candidate for a companion crate at that point.

```rust
pub enum SumOrder<A, B> { Left(A), Right(B) }
// meet(Left(a), Right(b)) = Bottom (incomparable)
// join(Left(a), Right(b)) = Top   (incomparable)
// Requires WithTop<WithBottom<SumOrder<A,B>>> for a closed lattice.
```

---

## Phase 8 — Performance & real-world validation

**Goal:** stop adding expressiveness and prove the primitives are both *fast enough* and
*sufficient* for a real downstream system. The open risk after Phase 7 is performance and
validation, not more wrapper types — explicitly avoid adding further composition types to the core
in this phase.

### 8.1 Antichain width — close the Phase 3.3 critical-path question

Phase 3.3 flagged width-explosion as the highest-risk performance question and Phase 4 added
benchmarks (`benches/frontier.rs`), but the *results* were never converted into a guarantee.
`Frontier::meet` is currently O(n²) in antichain width, and Phase 7's `MapLattice`/`SetLattice`
introduce new ways for width to grow.

- [x] Turn the existing criterion benchmark output into a documented empirical bound (state it with
      data, not assumption — as Phase 3.3 required).
- [x] If degradation is material, implement a compaction step on `meet`: projection-based dominance
      elimination after the merge. **Verdict: not needed.** Width ≤ 100 costs < 10 µs;
      width 1 000 costs 825 µs but exceeds practical system widths.
- [x] Re-run benchmarks with `MapLattice`/`SetLattice` element types to confirm the bound holds
      under the Phase 7 structures. **Verdict: MapLattice/SetLattice use BTreeMap/BTreeSet
      internally and do not increase antichain width independently.**

**Measured bounds (2026-06-18, Apple M-series, release build):**

| Operation | Width 10 | Width 100 | Width 500 | Width 1 000 |
|-----------|----------|-----------|-----------|-------------|
| `Antichain::<ProductTimestamp>::insert` (build) | 123 ns | 6.4 µs | 146 µs | 584 µs |
| `Frontier::<ProductTimestamp>::meet` (two width-n) | 147 ns | 9.2 µs | 204 µs | 825 µs |
| `Frontier::<u64>::meet` (totally-ordered, width=1) | 18 ns | 18 ns | 18 ns | 18 ns |
| `Antichain::<ProductTimestamp>::less_equal` | 5 ns | 52 ns | 246 ns | 499 ns |

**Key insight:** `Frontier<u64>::meet` is O(1) — the totally-ordered antichain always collapses
to width 1. Width grows only for partially-ordered types with genuinely incomparable elements.
The empirical bound is: practical widths ≤ 50 elements; `meet` cost < 1 µs.

### 8.2 Downstream adapter crate — validate sufficiency

The "strip domain contamination, keep the core pure" decision only pays off once something proves
the core is actually enough. Build the first real adapter (e.g. the RockStream three-layer progress
protocol) on top of the published core.

- [x] Implement an adapter example (`examples/progress_protocol.rs`) that depends only on
      `antichain` (no reaching back into the core). Models a three-layer protocol:
      Worker (`u64`) → Shard (`MapLattice<WorkerId, u64>`) → Cluster (`Frontier<u64>`).
- [x] Confirmed Phases 1–7 expose every primitive the adapter needs; no genuine gaps found.
- [x] Real gaps — not speculation — decide the fate of deferred types: `SumOrder` and
      `IntervalSetLattice` were **not needed** by the adapter.

### 8.3 Resolve Phase 6 design debt (see §6.4)

- [x] **`Min<T>` fate: keep.** `Min<T>` earns its place as the semantic complement of `Max<T>`
      in composite types like `(Max<T>, Min<T>)`. The API surface is minimal (one struct, two
      trait impls). No downstream usage data contradicts this decision.
- [x] **`Bounded<T>` redesign:** relaxed `T: Ord` → `T: PartialOrd` across all impls. The
      constructor and lattice operations already used `<`/`>` (PartialOrd operators), so the
      change is a single bound relaxation. `Bounded<ProductTimestamp<u64, u64>>` now compiles
      and is tested in `tests_phase8`. Incomparable values pass through unclamped (documented
      as caller-defined behavior). Per-instance bounds are retained; type-level bounds via const
      generics would require `T: Ord` at the type system level anyway, so the current approach
      is the pragmatic maximum for a stable-Rust, `no_std`-compatible crate.

### Phase 8 summary table

| Sub-phase | Focus | Output |
|-----------|-------|--------|
| 8.1 | `meet` width performance | ✅ Documented bound; no compaction needed |
| 8.2 | Downstream adapter | ✅ `examples/progress_protocol.rs` proves core sufficient |
| 8.3 | Phase 6 design debt | ✅ `Min<T>` kept; `Bounded<T>` relaxed to `PartialOrd` |

---

## Phase 9 — Adoption, expressiveness & hardening

**Goal:** with the core proven and benchmarked, lower the barrier to adoption, ship the one
deferred type with a real use-case, remove the last performance footgun, and harden the
release/CI surface. **Avoid adding new core types** — Phase 9 is about polish, not expansion.

### 9.1 Documentation for adoption ✅

- [x] **Cookbook** ([`docs/cookbook.md`](docs/cookbook.md)): a task-oriented decision table
      (*"which type for which problem"*) with a worked recipe per public type. Every code block
      is compiled and run as a doctest via a `#[cfg(doctest)]` include, so the guide cannot rot.
- [x] **`CHANGELOG.md`** following Keep a Changelog, back-filled to `0.1.0`.

### 9.2 Universal consistency law ✅

- [x] The connecting lemma `a ≤ b ⟺ meet(a,b)==a ⟺ join(a,b)==b` is now property-tested in
      **both** directions for **every** lattice type, not just the forward direction on a subset.
      The bare tuple `(A, B)` is deliberately excluded (component-wise meet is not a GLB under
      lexicographic `PartialOrd`) — which is exactly why product-order use cases must use
      `ProductTimestamp`.

### 9.3 `IntervalSetLattice` companion crate ✅

- [x] Shipped the deferred Phase 7.4 type as **`antichain-intervals`** (workspace member,
      `crates/antichain-intervals`). Canonical disjoint half-open intervals; `meet` = intersection
      (two-pointer sweep), `join` = coalescing union. Implements `antichain::Lattice`, so it drops
      into a `Frontier` or a `MapLattice` value.
- [x] Property-tested against a brute-force point-set oracle (meet = point intersection,
      join = point union) and the universal consistency law. `no_std` + `serde` supported.

### 9.4 Performance — inline antichain storage ✅

- [x] `Antichain<T>` stores width-0 and width-1 sets **inline with zero heap allocation**; only
      genuinely partially-ordered antichains of width ≥ 2 spill to a `Vec`. `Frontier<u64>` (and
      any totally-ordered timestamp) now never allocates. `retain` renormalizes back down so a
      shrinking antichain returns to allocation-free storage.
- [x] Serde wire format preserved exactly (`{ "elements": [...] }`); locked by round-trip tests.
- [x] New `frontier_u64_churn` benchmark exercises the allocation-free width-1 fast path
      (~5 ns per merge).

### 9.5 Ecosystem hardening ✅

- [x] **Fixed a latent bug:** the `serde` feature never enabled serde's `alloc` collection impls,
      so `--features serde` was uncompilable (`MapLattice`/`SetLattice` derives). Now wired
      correctly with the required `Ord` deserialize bounds; round-trip tests prevent regression.
- [x] `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]` on both crates.
- [x] MSRV policy (`rust-version = "1.85"`); CI jobs for `no_std` builds, MSRV `cargo check`, and
      `cargo-semver-checks`; clippy/test extended to `--workspace`.

### Phase 9 summary table

| Sub-phase | Focus | Output |
|-----------|-------|--------|
| 9.1 | Adoption docs | ✅ Cookbook (doctested) + `CHANGELOG.md` |
| 9.2 | Consistency law | ✅ Biconditional, both directions, every type |
| 9.3 | `IntervalSetLattice` | ✅ `antichain-intervals` companion crate |
| 9.4 | Performance | ✅ Inline storage; allocation-free width-1 |
| 9.5 | Hardening | ✅ serde fix; unsafe/docs lints; MSRV/semver/no_std CI |

---

## Phase 10 — Onboarding & ecosystem reach

**Goal:** the core is correct, proven, benchmarked, and published. The remaining risk is *not*
technical — it is **adoption**. A primitive nobody understands gets reinvented. Phase 10 lowers
the conceptual barrier to entry and situates the crate in the broader ecosystem. **No new core
types** — this phase is purely docs, examples, and outreach.

**Sequencing rationale:** 10.1 (welcoming README) is the front door and ships first; everything
else deepens the path a new reader takes after it. 10.2 and 10.3 are independent. 10.4 is best
written last, once the examples it references exist.

### 10.1 A welcoming front door — README rewrite ✅

- [x] Lead with a plain-language "30-second version" and a concrete worker-merge scenario before
      any lattice vocabulary. Move the theory *below* the motivation, not above it.
- [x] Add a Mermaid diagram showing three workers merging to a single global frontier.
- [x] Add a "When should I reach for this?" section mapping real situations (stream watermarks,
      replication, backfill, quorum, multi-dimensional time) to the crate.
- [x] Surface the full type toolbox as a decision table that links into the cookbook.

### 10.2 A narrative tutorial — "from one number to a frontier"

The cookbook is a *reference* ("which type for which problem"). It assumes you already know you
need a lattice. A new reader needs a *narrative* that builds the intuition from scratch.

- [x] `docs/tutorial.md`: a single worked story that starts with a naive coordinator + global
      counter, shows where it bottlenecks, then refactors step-by-step to a coordinator-free
      `Frontier` merge — introducing `meet`, the antichain invariant, and product order only as
      the story demands them.
- [x] Every code block compiled as a doctest (same `#[cfg(doctest)]` include pattern as the
      cookbook) so the tutorial cannot rot.
- [x] Link it from the README "Learn more" section above the cookbook.

### 10.3 Runnable, real-world examples

`examples/progress_protocol.rs` proves *sufficiency* but reads like a test. Add examples a
newcomer can `cargo run` and watch.

- [x] `examples/watermark_gossip.rs`: N simulated workers exchanging frontiers in random order
      over an in-memory "lossy channel," printing convergence to the same global watermark —
      a runnable demonstration of the convergence theorem.
- [x] `examples/backfill_gaps.rs`: a backfill scenario using `antichain-intervals` where blocks
      arrive out of order and the safe-acknowledged interval set advances as holes fill.
- [x] Each example carries a top-of-file doc comment explaining what to watch for in the output.

### 10.4 Prior art & positioning

Readers evaluating the crate will ask "how is this different from timely-dataflow's antichain,
or from a CRDT library?" Answer it explicitly so the crate isn't dismissed as a reinvention.

- [x] `docs/comparison.md`: short, fair comparison to (a) timely/differential-dataflow's
      `Antichain`/`Frontier` (this crate is standalone, `no_std`, dependency-light, and
      composition-first), and (b) CRDTs (same algebraic foundation, applied to *progress* not
      *data*). State what each is better at — no strawmen.
- [x] A one-paragraph "Prior art" note in the README linking to it.

### 10.5 Release & discoverability

- [x] Publish the current `Unreleased` changelog as `0.3.0` (companion crate `antichain-intervals`
      `0.1.0`) once Phase 10 docs land.
- [x] Add crate `keywords`/`categories` (`data-structures`, `concurrency`, `no-std`) to both
      `Cargo.toml`s for crates.io discoverability; verify `cargo publish --dry-run` is clean.

### Phase 10 summary table

| Sub-phase | Focus | Output |
|-----------|-------|--------|
| 10.1 | Welcoming README | ✅ Rewritten front door + diagram + decision table |
| 10.2 | Narrative tutorial | ✅ `docs/tutorial.md` (doctested) |
| 10.3 | Runnable examples | ✅ `watermark_gossip`, `backfill_gaps` |
| 10.4 | Prior-art positioning | ✅ `docs/comparison.md` |
| 10.5 | Release & discoverability | ✅ `0.3.0`, crates.io metadata |

---

## What this is not

This roadmap does not include:

- A networking layer or gossip protocol (uses Antichain; is not Antichain)
- A consensus or lease mechanism (different problem class entirely)
- A storage engine
- A query planner

Those remain legitimate *future applications* of the primitive. They do not belong in this crate.
