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

---

## What this is not

This roadmap does not include:

- A networking layer or gossip protocol (uses Antichain; is not Antichain)
- A consensus or lease mechanism (different problem class entirely)
- A storage engine
- A query planner

Those remain legitimate *future applications* of the primitive. They do not belong in this crate.
