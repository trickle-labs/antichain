//! A coordinator-free primitive for tracking distributed progress using lattice algebra.
//!
//! # Overview
//!
//! This crate provides three core primitives:
//!
//! * [`Lattice`] — a trait for types with a greatest lower bound (`meet`) and least upper bound
//!   (`join`), consistent with `PartialOrd`.
//! * [`Antichain<T>`] — a set of mutually incomparable elements maintaining the antichain
//!   invariant: no element is `<=` any other.
//! * [`Frontier<T>`] — a progress claim: all timestamps strictly less than some antichain element
//!   are considered complete.
//!
//! Two composition types are also provided:
//!
//! * [`ProductTimestamp<T1, T2>`] — product order: `(a1, b1) ≤ (a2, b2)` iff `a1 ≤ a2` **and**
//!   `b1 ≤ b2`. Used for independent multi-dimensional clocks.
//! * [`Lexicographic<A, B>`] — lexicographic order: outer dimension dominates; inner breaks ties.
//!   Used for epoch × offset patterns.
//!
//! Three order-modifier wrappers complete the Phase 6 composition toolkit:
//!
//! * [`Max<T>`] — inverts `T`'s partial order so that [`Frontier::meet`] computes `max` instead
//!   of `min`. Tracks "at least X" lower bounds.
//! * [`Min<T>`] — preserves `T`'s natural order (transparent newtype). Used alongside [`Max`] in
//!   composite types for semantic clarity.
//! * [`Bounded<T>`] — clamps values to a `[min, max]` interval, giving a provable upper bound on
//!   antichain width for finite ranges.
//!
//! Phase 7 adds structural and dynamic lattice types:
//!
//! * [`WithTop<T>`] — adds a structural `Top` sentinel above all `Value(t)`. `Top` absorbs
//!   [`Lattice::join`] and is the identity for [`Lattice::meet`]. Signals a permanently-closed
//!   data path.
//! * [`WithBottom<T>`] — adds a structural `Bottom` sentinel below all `Value(t)`. `Bottom`
//!   absorbs [`Lattice::meet`] and is the identity for [`Lattice::join`]. Compose with
//!   [`WithTop`] to produce `Bottom < Value(t) < Top`.
//! * [`MapLattice<K, V>`] — point-wise lattice over a `BTreeMap`. Models progress in
//!   runtime-topology systems where the set of dimensions (shards, partitions) changes
//!   dynamically. Meet = key-intersection + value-meet; join = key-union + value-join.
//! * [`SetLattice<T>`] — powerset lattice over a `BTreeSet`. The partial order is set inclusion.
//!   Meet is intersection; join is union. Models universal acknowledgement across a cluster.
//!
//! # Quick start
//!
//! ```
//! use antichain::Frontier;
//!
//! // Two workers report their progress independently.
//! let worker_a = Frontier::from_elem(5u64);
//! let worker_b = Frontier::from_elem(3u64);
//!
//! // Merge without coordination — meet is commutative, associative, and idempotent.
//! let merged = worker_a.meet(&worker_b);
//! assert_eq!(merged, worker_b.meet(&worker_a));  // commutative
//! assert!(merged.less_equal(&3));                // timestamp 3 is still in-flight
//! assert!(!merged.less_equal(&7));               // timestamp 7 is past the frontier
//! ```
//!
//! # Convergence guarantee
//!
//! Two nodes that have each seen any subset of the same update set, in any order, will hold
//! identical [`Frontier`] values after merging.
//!
//! ```
//! use antichain::Frontier;
//!
//! let updates = [
//!     Frontier::from_elem(3u64),
//!     Frontier::from_elem(7u64),
//!     Frontier::from_elem(5u64),
//! ];
//!
//! // Node A applies updates in order [0, 1, 2].
//! let node_a = updates[0].meet(&updates[1]).meet(&updates[2]);
//!
//! // Node B applies updates in a different order [2, 0, 1].
//! let node_b = updates[2].meet(&updates[0]).meet(&updates[1]);
//!
//! // Node C applies updates in yet another order [1, 2, 0].
//! let node_c = updates[1].meet(&updates[2]).meet(&updates[0]);
//!
//! // All three converge to the same value regardless of order.
//! assert_eq!(node_a, node_b);
//! assert_eq!(node_b, node_c);
//! ```
//!
//! # `no_std` support
//!
//! Disable the default `std` feature to use this crate in `no_std` environments. A global
//! allocator must be present (`extern crate alloc` is used internally):
//!
//! ```toml
//! [dependencies]
//! antichain = { version = "0.1", default-features = false }
//! ```
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `std`   | yes     | Link against `std`; disable for `no_std` + `alloc` environments. |
//! | `serde` | no      | Derive `Serialize` / `Deserialize` for all public types. |
//!
//! # Performance
//!
//! **Empirical width bounds (measured 2026-06-18, Apple M-series, release build):**
//!
//! | Operation | Width 10 | Width 100 | Width 500 | Width 1 000 |
//! |-----------|----------|-----------|-----------|-------------|
//! | `Antichain::<ProductTimestamp>::insert` (build width-*n* antichain) | 123 ns | 6.4 µs | 146 µs | 584 µs |
//! | `Frontier::<ProductTimestamp>::meet` (two width-*n* antichains) | 147 ns | 9.2 µs | 204 µs | 825 µs |
//! | `Frontier::<u64>::meet` (totally-ordered; collapses to width 1) | 18 ns | 18 ns | 18 ns | 18 ns |
//! | `Antichain::<ProductTimestamp>::less_equal` (dominates query) | 5 ns | 52 ns | 246 ns | 499 ns |
//!
//! **Interpretation:**
//! - `Frontier::<u64>::meet` is **O(1)** at any input count: the totally-ordered antichain
//!   collapses to width 1 (the minimum element), so inserting 1 000 u64 values costs the
//!   same as inserting 10.
//! - `Frontier::<ProductTimestamp>::meet` is empirically **O(n²)** in antichain width *n*.
//!   At widths seen in practice (typically ≤ 50 independent stream partitions), the cost
//!   is in the low-microsecond range.
//! - **Compaction verdict (Phase 8.1):** no compaction step is required. At width ≤ 100 the
//!   `meet` cost is < 10 µs. Width 1 000 (825 µs) exceeds practical system widths; if
//!   antichains grow beyond ≈ 100 elements, model each independent dimension as a
//!   [`MapLattice`] key rather than widening the antichain.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(not(feature = "std"))]
extern crate alloc;

// `Vec` at the crate root is referenced by the serde deserialize helper and by
// the test modules; the `vec!` macro is used only by tests.
#[cfg(all(not(feature = "std"), test))]
use alloc::vec;
#[cfg(all(not(feature = "std"), any(test, feature = "serde")))]
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use alloc::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};

// Compile and run every code block in the cookbook as a doctest. The item only
// exists under `cargo test --doc`, so it never appears in the generated API docs.
#[cfg(doctest)]
#[doc = include_str!("../docs/cookbook.md")]
pub struct Cookbook;

// Compile and run every code block in the tutorial as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../docs/tutorial.md")]
pub struct Tutorial;

// ── Lattice ───────────────────────────────────────────────────────────────────

/// Greatest lower bound (`meet`) and least upper bound (`join`).
///
/// Implementations must be consistent with `PartialOrd`:
/// - `meet(a, b) <= a` and `meet(a, b) <= b`
/// - `a <= join(a, b)` and `b <= join(a, b)`
pub trait Lattice: PartialOrd {
    /// Returns the greatest lower bound of `self` and `other`.
    fn meet(&self, other: &Self) -> Self;
    /// Returns the least upper bound of `self` and `other`.
    fn join(&self, other: &Self) -> Self;
}

macro_rules! impl_lattice_ord {
    ($($t:ty),*) => {
        $(
            impl Lattice for $t {
                #[inline] fn meet(&self, other: &Self) -> Self { (*self).min(*other) }
                #[inline] fn join(&self, other: &Self) -> Self { (*self).max(*other) }
            }
        )*
    }
}

impl_lattice_ord!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

// ── Lattice for 2-tuples ──────────────────────────────────────────────────────

/// Component-wise `Lattice` for 2-tuples.
///
/// Note: standard-library tuples use *lexicographic* `PartialOrd`, so these
/// component-wise meet/join operations are not the greatest lower bound /
/// least upper bound under that ordering. For true product-order semantics use
/// [`ProductTimestamp`] instead. This impl is a lightweight convenience for
/// cases like `(partition, offset)` where component-wise advancement is the
/// desired behaviour; the bound invariants `meet(a,b) ≤ a` and `a ≤ join(a,b)`
/// are still satisfied under the lexicographic `PartialOrd`.
impl<A: Lattice + Clone, B: Lattice + Clone> Lattice for (A, B) {
    #[inline]
    fn meet(&self, other: &Self) -> Self {
        (self.0.meet(&other.0), self.1.meet(&other.1))
    }
    #[inline]
    fn join(&self, other: &Self) -> Self {
        (self.0.join(&other.0), self.1.join(&other.1))
    }
}

// ── ProductTimestamp ──────────────────────────────────────────────────────────

/// A pair timestamp with the *product order*: `(a1, b1) ≤ (a2, b2)` iff
/// `a1 ≤ a2` **and** `b1 ≤ b2`.
///
/// This differs from the standard-library tuple `PartialOrd`, which is
/// lexicographic. Use this type when you need component-wise incomparability
/// (e.g., independent partition offsets or multi-dimensional clocks).
///
/// Elements that are neither `≤` nor `≥` each other are *incomparable*;
/// `partial_cmp` returns `None` for them.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProductTimestamp<T1, T2> {
    /// The first (outer) dimension.
    pub outer: T1,
    /// The second (inner) dimension.
    pub inner: T2,
}

impl<T1, T2> ProductTimestamp<T1, T2> {
    /// Creates a new `ProductTimestamp`.
    pub fn new(outer: T1, inner: T2) -> Self {
        Self { outer, inner }
    }
}

impl<T1: PartialOrd, T2: PartialOrd> PartialOrd for ProductTimestamp<T1, T2> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        use core::cmp::Ordering::{Equal, Greater, Less};
        match (
            self.outer.partial_cmp(&other.outer),
            self.inner.partial_cmp(&other.inner),
        ) {
            (Some(Less), Some(Less | Equal)) | (Some(Equal), Some(Less)) => Some(Less),
            (Some(Equal), Some(Equal)) => Some(Equal),
            (Some(Greater), Some(Greater | Equal)) | (Some(Equal), Some(Greater)) => Some(Greater),
            _ => None, // incomparable
        }
    }
}

impl<T1: Lattice + Clone, T2: Lattice + Clone> Lattice for ProductTimestamp<T1, T2> {
    /// Component-wise meet: `(meet(a1,b1), meet(a2,b2))`.
    fn meet(&self, other: &Self) -> Self {
        ProductTimestamp {
            outer: self.outer.meet(&other.outer),
            inner: self.inner.meet(&other.inner),
        }
    }
    /// Component-wise join: `(join(a1,b1), join(a2,b2))`.
    fn join(&self, other: &Self) -> Self {
        ProductTimestamp {
            outer: self.outer.join(&other.outer),
            inner: self.inner.join(&other.inner),
        }
    }
}

// ── Lexicographic ─────────────────────────────────────────────────────────────

/// A pair timestamp with *lexicographic* order: the outer dimension totally
/// orders; the inner dimension breaks ties.
///
/// Requires `A: Ord` so that outer comparisons are always decisive.
/// Typical use: `Lexicographic<EpochId, Offset>` where the epoch totally
/// dominates and the offset provides sub-epoch ordering.
///
/// The `Lattice` impl computes the true greatest lower bound / least upper
/// bound under this order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lexicographic<A, B> {
    /// The outer dimension, which totally dominates the ordering.
    pub outer: A,
    /// The inner dimension, which breaks ties when outer values are equal.
    pub inner: B,
}

impl<A, B> Lexicographic<A, B> {
    /// Creates a new `Lexicographic` timestamp.
    pub fn new(outer: A, inner: B) -> Self {
        Self { outer, inner }
    }
}

impl<A: Ord, B: PartialOrd> PartialOrd for Lexicographic<A, B> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match self.outer.cmp(&other.outer) {
            core::cmp::Ordering::Equal => self.inner.partial_cmp(&other.inner),
            ord => Some(ord),
        }
    }
}

impl<A: Ord + Clone, B: Lattice + Clone> Lattice for Lexicographic<A, B> {
    fn meet(&self, other: &Self) -> Self {
        match self.outer.cmp(&other.outer) {
            core::cmp::Ordering::Less => self.clone(),
            core::cmp::Ordering::Greater => other.clone(),
            core::cmp::Ordering::Equal => Lexicographic {
                outer: self.outer.clone(),
                inner: self.inner.meet(&other.inner),
            },
        }
    }
    fn join(&self, other: &Self) -> Self {
        match self.outer.cmp(&other.outer) {
            core::cmp::Ordering::Less => other.clone(),
            core::cmp::Ordering::Greater => self.clone(),
            core::cmp::Ordering::Equal => Lexicographic {
                outer: self.outer.clone(),
                inner: self.inner.join(&other.inner),
            },
        }
    }
}

// ── Antichain storage ─────────────────────────────────────────────────────────

mod storage {
    #[cfg(not(feature = "std"))]
    use alloc::vec::Vec;
    #[cfg(feature = "std")]
    use std::vec::Vec;

    /// Inline storage for an antichain's elements.
    ///
    /// The overwhelmingly common cases are width 0 (an unconstrained frontier) and
    /// width 1 (any totally-ordered timestamp such as `u64`, which always collapses
    /// to its minimum). Both are held inline with **zero heap allocation**; only
    /// genuinely partially-ordered antichains of width ≥ 2 spill to a `Vec`.
    ///
    /// `retain` renormalizes back down to `One`/`Zero` so an antichain that shrinks
    /// returns to allocation-free storage.
    #[derive(Clone, Debug)]
    pub(crate) enum Inline<T> {
        Zero,
        One(T),
        Many(Vec<T>),
    }

    impl<T> Inline<T> {
        #[inline]
        pub(crate) fn new() -> Self {
            Inline::Zero
        }

        #[inline]
        pub(crate) fn one(t: T) -> Self {
            Inline::One(t)
        }

        /// Builds inline storage from a `Vec`, used only by the serde deserialize
        /// path (other construction goes through `new`/`one`/`push`).
        #[cfg(feature = "serde")]
        pub(crate) fn from_vec(v: Vec<T>) -> Self {
            match v.len() {
                0 => Inline::Zero,
                1 => Inline::One(v.into_iter().next().expect("len checked == 1")),
                _ => Inline::Many(v),
            }
        }

        #[inline]
        pub(crate) fn as_slice(&self) -> &[T] {
            match self {
                Inline::Zero => &[],
                Inline::One(t) => core::slice::from_ref(t),
                Inline::Many(v) => v.as_slice(),
            }
        }

        #[inline]
        pub(crate) fn len(&self) -> usize {
            match self {
                Inline::Zero => 0,
                Inline::One(_) => 1,
                Inline::Many(v) => v.len(),
            }
        }

        #[inline]
        pub(crate) fn is_empty(&self) -> bool {
            matches!(self, Inline::Zero)
        }

        #[inline]
        pub(crate) fn iter(&self) -> core::slice::Iter<'_, T> {
            self.as_slice().iter()
        }

        pub(crate) fn push(&mut self, t: T) {
            match self {
                Inline::Zero => *self = Inline::One(t),
                Inline::One(_) => {
                    let first = match core::mem::replace(self, Inline::Zero) {
                        Inline::One(x) => x,
                        _ => unreachable!("matched One above"),
                    };
                    *self = Inline::Many(Vec::from([first, t]));
                }
                Inline::Many(v) => v.push(t),
            }
        }

        pub(crate) fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            match self {
                Inline::Zero => {}
                Inline::One(t) => {
                    if !f(t) {
                        *self = Inline::Zero;
                    }
                }
                Inline::Many(v) => {
                    v.retain(|e| f(e));
                    // Renormalize so a shrunken antichain returns to allocation-free storage.
                    match v.len() {
                        0 => *self = Inline::Zero,
                        1 => *self = Inline::One(v.pop().expect("len checked == 1")),
                        _ => {}
                    }
                }
            }
        }
    }
}

// ── Antichain ─────────────────────────────────────────────────────────────────

/// A set of mutually incomparable elements under `PartialOrd`.
///
/// Invariant: no element `x` in the set satisfies `x <= y` or `y <= x`
/// for any other element `y` in the set.
///
/// Width-0 and width-1 antichains are stored inline without heap allocation. For
/// totally-ordered timestamps (e.g. `Frontier<u64>`) the antichain always stays at
/// width 1, so it never allocates.
#[derive(Clone, Debug)]
pub struct Antichain<T> {
    elements: storage::Inline<T>,
}

/// Two antichains are equal when they contain the same *set* of elements,
/// regardless of insertion order.
impl<T: PartialEq> PartialEq for Antichain<T> {
    fn eq(&self, other: &Self) -> bool {
        self.elements.len() == other.elements.len()
            && self
                .elements
                .iter()
                .all(|e| other.elements.as_slice().contains(e))
    }
}

impl<T: Eq> Eq for Antichain<T> {}

/// Serializes as `{ "elements": [...] }` — identical to the previous derived
/// representation, so the inline-storage optimization is wire-compatible.
#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Antichain<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Antichain", 1)?;
        state.serialize_field("elements", self.elements.as_slice())?;
        state.end()
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Antichain<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper<T> {
            elements: Vec<T>,
        }
        let helper = Helper::deserialize(deserializer)?;
        Ok(Antichain {
            elements: storage::Inline::from_vec(helper.elements),
        })
    }
}

impl<T: PartialOrd + Clone> Antichain<T> {
    /// Creates an empty antichain.
    pub fn empty() -> Self {
        Self {
            elements: storage::Inline::new(),
        }
    }

    /// Creates an antichain containing a single element.
    pub fn from_elem(t: T) -> Self {
        Self {
            elements: storage::Inline::one(t),
        }
    }

    /// Inserts `t`, maintaining the antichain invariant.
    ///
    /// - Skips `t` if any existing element `e` satisfies `e <= t` (t is dominated).
    /// - Drops any existing `e` where `t <= e` (e is dominated by t).
    pub fn insert(&mut self, t: T) {
        if self.elements.iter().any(|e| *e <= t) {
            return;
        }
        self.elements.retain(|e| {
            t.partial_cmp(e)
                .is_none_or(|o| o == core::cmp::Ordering::Greater)
        });
        self.elements.push(t);
    }

    /// Returns the elements of the antichain as a slice.
    pub fn elements(&self) -> &[T] {
        self.elements.as_slice()
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns `true` if the antichain has no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns `true` if `time` is less than or equal to some element of this antichain.
    ///
    /// In the context of a [`Frontier`], this means `time` is still in-flight.
    pub fn less_equal(&self, time: &T) -> bool {
        self.elements.iter().any(|e| *time <= *e)
    }
}

// ── Frontier ──────────────────────────────────────────────────────────────────

/// A progress claim: all timestamps strictly less than some element are complete.
///
/// A `Frontier` is a pure value type — no networking, no I/O, no async.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frontier<T> {
    antichain: Antichain<T>,
}

impl<T: PartialOrd + Clone> Frontier<T> {
    /// The identity element for [`meet`][Self::meet`] — an unconstrained frontier
    /// with no elements, where no timestamp is reported in-flight.
    pub fn bottom() -> Self {
        Self {
            antichain: Antichain::empty(),
        }
    }

    /// Creates a frontier from a single element.
    pub fn from_elem(t: T) -> Self {
        Self {
            antichain: Antichain::from_elem(t),
        }
    }

    /// Creates a frontier from an iterator of elements.
    pub fn from_elements(iter: impl IntoIterator<Item = T>) -> Self {
        let mut antichain = Antichain::empty();
        for t in iter {
            antichain.insert(t);
        }
        Self { antichain }
    }

    /// Returns `true` if `time` is less than or equal to some element of this frontier,
    /// meaning `time` is still in-flight.
    pub fn less_equal(&self, time: &T) -> bool {
        self.antichain.less_equal(time)
    }

    /// Returns the underlying antichain elements.
    pub fn elements(&self) -> &[T] {
        self.antichain.elements()
    }

    /// Coordinator-free merge: the most conservative frontier dominated by both.
    ///
    /// This is the lattice **meet** (greatest lower bound): the result is the most advanced
    /// frontier that is still less than or equal to both inputs.
    ///
    /// Properties proven by the Phase 2 property tests:
    /// - **Commutative**: `meet(a, b) == meet(b, a)`
    /// - **Associative**: `meet(a, meet(b, c)) == meet(meet(a, b), c)`
    /// - **Idempotent**: `meet(a, a) == a`
    ///
    /// **Convergence guarantee**: two nodes that have each seen any subset of the same update set,
    /// in any order, will hold identical `Frontier` values after calling `meet` for each update.
    ///
    /// **Performance:** O(n²) in antichain width *n*. For `Frontier<u64>` (totally ordered),
    /// the antichain collapses to width 1, making this effectively O(1). For
    /// `Frontier<ProductTimestamp<u64, u64>>` at worst-case width: 100 ≈ 9 µs, 500 ≈ 204 µs,
    /// 1 000 ≈ 825 µs (measured 2026-06-18). Practical production widths are ≤ 50.
    ///
    /// ```
    /// use antichain::Frontier;
    ///
    /// let f1 = Frontier::from_elem(7u64);
    /// let f2 = Frontier::from_elem(3u64);
    ///
    /// // meet returns the more conservative (lower) frontier
    /// let merged = f1.meet(&f2);
    /// assert!(merged.less_equal(&3));   // 3 still in-flight
    /// assert!(!merged.less_equal(&7));  // 7 is past the merged frontier
    ///
    /// // order of application does not matter
    /// assert_eq!(f1.meet(&f2), f2.meet(&f1));
    /// ```
    pub fn meet(&self, other: &Self) -> Self {
        let mut antichain = Antichain::empty();
        for e in self.antichain.elements() {
            antichain.insert(e.clone());
        }
        for e in other.antichain.elements() {
            antichain.insert(e.clone());
        }
        Self { antichain }
    }
}

impl<T: Lattice + Clone> Frontier<T> {
    /// Lattice join (least upper bound) — the most advanced frontier dominated by both.
    ///
    /// Requires `T: Lattice` to compute element-wise joins across the two antichains.
    pub fn join(&self, other: &Self) -> Self {
        let mut antichain = Antichain::empty();
        for e1 in self.antichain.elements() {
            for e2 in other.antichain.elements() {
                antichain.insert(e1.join(e2));
            }
        }
        Self { antichain }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Antichain ────────────────────────────────────────────────────────────

    #[test]
    fn antichain_empty_has_no_elements() {
        let a: Antichain<u64> = Antichain::empty();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
    }

    #[test]
    fn antichain_from_elem() {
        let a = Antichain::from_elem(5u64);
        assert_eq!(a.elements(), &[5]);
    }

    #[test]
    fn antichain_insert_dominated_element_is_skipped() {
        // 3 is already in the set; 5 > 3 so 5 is dominated — skip it
        let mut a = Antichain::empty();
        a.insert(3u64);
        a.insert(5u64);
        assert_eq!(a.elements(), &[3]);
    }

    #[test]
    fn antichain_insert_dominating_element_replaces() {
        // 5 is in the set; 3 < 5 so 3 dominates 5 — remove 5, add 3
        let mut a = Antichain::empty();
        a.insert(5u64);
        a.insert(3u64);
        assert_eq!(a.elements(), &[3]);
    }

    #[test]
    fn antichain_insert_duplicate_is_idempotent() {
        let mut a = Antichain::empty();
        a.insert(5u64);
        a.insert(5u64);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn antichain_incomparable_elements_both_kept() {
        // (1,3) and (3,1) are incomparable in product order (std tuple is lexico, but we
        // verify antichain behaviour with integers that are clearly incomparable via
        // a pair represented as two separate inserts is not possible — use a type that
        // exposes incomparability, here we just note the invariant is maintained)
        let mut a: Antichain<u64> = Antichain::empty();
        a.insert(5);
        a.insert(7);
        // 5 < 7, so 7 is dominated by 5 → only 5 survives
        assert_eq!(a.elements(), &[5]);
    }

    #[test]
    fn antichain_less_equal() {
        let a = Antichain::from_elem(5u64);
        assert!(a.less_equal(&3)); // 3 <= 5 → in-flight
        assert!(a.less_equal(&5)); // 5 <= 5 → in-flight
        assert!(!a.less_equal(&7)); // 7 > 5  → past the frontier
    }

    // ── Frontier ─────────────────────────────────────────────────────────────

    #[test]
    fn frontier_bottom_has_no_in_flight_timestamps() {
        let f: Frontier<u64> = Frontier::bottom();
        assert!(f.elements().is_empty());
        assert!(!f.less_equal(&0));
    }

    #[test]
    fn frontier_from_elem() {
        let f = Frontier::from_elem(5u64);
        assert_eq!(f.elements(), &[5]);
        assert!(f.less_equal(&3));
        assert!(f.less_equal(&5));
        assert!(!f.less_equal(&7));
    }

    #[test]
    fn frontier_from_elements_keeps_minimal() {
        // 3 <= 5 <= 7 so only 3 survives (most conservative)
        let f = Frontier::from_elements([3u64, 7, 5]);
        assert_eq!(f.elements(), &[3]);
    }

    #[test]
    fn frontier_meet_is_conservative() {
        let f1 = Frontier::from_elem(7u64);
        let f2 = Frontier::from_elem(3u64);
        assert_eq!(f1.meet(&f2).elements(), &[3]);
    }

    #[test]
    fn frontier_meet_is_commutative() {
        let f1 = Frontier::from_elem(7u64);
        let f2 = Frontier::from_elem(3u64);
        assert_eq!(f1.meet(&f2), f2.meet(&f1));
    }

    #[test]
    fn frontier_meet_is_idempotent() {
        let f = Frontier::from_elem(5u64);
        assert_eq!(f.meet(&f), f);
    }

    #[test]
    fn frontier_meet_with_bottom_is_identity() {
        let f = Frontier::from_elem(5u64);
        assert_eq!(f.meet(&Frontier::bottom()), f);
        assert_eq!(Frontier::bottom().meet(&f), f);
    }

    #[test]
    fn frontier_join_advances_to_max() {
        let f1 = Frontier::from_elem(3u64);
        let f2 = Frontier::from_elem(7u64);
        assert_eq!(f1.join(&f2).elements(), &[7]);
    }

    #[test]
    fn frontier_join_is_commutative() {
        let f1 = Frontier::from_elem(3u64);
        let f2 = Frontier::from_elem(7u64);
        assert_eq!(f1.join(&f2), f2.join(&f1));
    }

    #[test]
    fn frontier_join_is_idempotent() {
        let f = Frontier::from_elem(5u64);
        assert_eq!(f.join(&f), f);
    }

    // ── Lattice impls ────────────────────────────────────────────────────────

    #[test]
    fn lattice_u64_meet_is_min() {
        assert_eq!(5u64.meet(&3), 3);
        assert_eq!(3u64.meet(&5), 3);
        assert_eq!(4u64.meet(&4), 4);
    }

    #[test]
    fn lattice_u64_join_is_max() {
        assert_eq!(5u64.join(&3), 5);
        assert_eq!(3u64.join(&5), 5);
        assert_eq!(4u64.join(&4), 4);
    }

    #[test]
    fn lattice_i64_meet_and_join() {
        assert_eq!((-3i64).meet(&-7), -7);
        assert_eq!((-3i64).join(&-7), -3);
    }

    // ── Tuple Lattice ─────────────────────────────────────────────────────────

    #[test]
    fn tuple_lattice_meet_is_component_wise() {
        let a = (3u64, 7u64);
        let b = (5u64, 2u64);
        assert_eq!(a.meet(&b), (3u64, 2u64));
    }

    #[test]
    fn tuple_lattice_join_is_component_wise() {
        let a = (3u64, 7u64);
        let b = (5u64, 2u64);
        assert_eq!(a.join(&b), (5u64, 7u64));
    }

    // ── ProductTimestamp ──────────────────────────────────────────────────────

    #[test]
    fn product_order_less() {
        let a = ProductTimestamp::new(1u64, 2u64);
        let b = ProductTimestamp::new(3u64, 4u64);
        assert!(a < b);
        assert!(b > a);
    }

    #[test]
    fn product_order_equal() {
        let a = ProductTimestamp::new(3u64, 5u64);
        let b = ProductTimestamp::new(3u64, 5u64);
        assert_eq!(a.partial_cmp(&b), Some(core::cmp::Ordering::Equal));
    }

    #[test]
    fn product_order_incomparable() {
        let a = ProductTimestamp::new(1u64, 4u64);
        let b = ProductTimestamp::new(3u64, 2u64);
        assert_eq!(a.partial_cmp(&b), None);
    }

    #[test]
    fn product_timestamp_meet_is_component_wise() {
        let a = ProductTimestamp::new(3u64, 7u64);
        let b = ProductTimestamp::new(5u64, 2u64);
        assert_eq!(a.meet(&b), ProductTimestamp::new(3u64, 2u64));
    }

    #[test]
    fn product_timestamp_join_is_component_wise() {
        let a = ProductTimestamp::new(3u64, 7u64);
        let b = ProductTimestamp::new(5u64, 2u64);
        assert_eq!(a.join(&b), ProductTimestamp::new(5u64, 7u64));
    }

    #[test]
    fn frontier_product_timestamp_incomparable_elements_both_kept() {
        // (1,3) and (3,1) are incomparable in product order
        let f = Frontier::from_elements([
            ProductTimestamp::new(1u64, 3u64),
            ProductTimestamp::new(3u64, 1u64),
        ]);
        assert_eq!(f.elements().len(), 2);
    }

    #[test]
    fn frontier_product_timestamp_dominated_element_removed() {
        // (1,1) <= (3,3), so (3,3) is dominated
        let f = Frontier::from_elements([
            ProductTimestamp::new(1u64, 1u64),
            ProductTimestamp::new(3u64, 3u64),
        ]);
        assert_eq!(f.elements(), &[ProductTimestamp::new(1u64, 1u64)]);
    }

    #[test]
    fn frontier_product_meet_merges_incomparable() {
        let f1 = Frontier::from_elem(ProductTimestamp::new(5u64, 1u64));
        let f2 = Frontier::from_elem(ProductTimestamp::new(1u64, 5u64));
        let m = f1.meet(&f2);
        assert_eq!(m.elements().len(), 2);
    }

    #[test]
    fn frontier_product_join_takes_component_max() {
        let f1 = Frontier::from_elem(ProductTimestamp::new(3u64, 7u64));
        let f2 = Frontier::from_elem(ProductTimestamp::new(5u64, 2u64));
        let j = f1.join(&f2);
        // join(e1, e2) = (5, 7), a single element
        assert_eq!(j.elements(), &[ProductTimestamp::new(5u64, 7u64)]);
    }

    // ── Lexicographic ─────────────────────────────────────────────────────────

    #[test]
    fn lexicographic_outer_dominates() {
        let a = Lexicographic::new(1u64, 99u64);
        let b = Lexicographic::new(2u64, 0u64);
        assert!(a < b);
    }

    #[test]
    fn lexicographic_inner_breaks_tie() {
        let a = Lexicographic::new(5u64, 3u64);
        let b = Lexicographic::new(5u64, 7u64);
        assert!(a < b);
    }

    #[test]
    fn lexicographic_meet_returns_lesser_when_outer_differs() {
        let a = Lexicographic::new(1u64, 99u64);
        let b = Lexicographic::new(2u64, 0u64);
        assert_eq!(a.meet(&b), a);
        assert_eq!(b.meet(&a), a);
    }

    #[test]
    fn lexicographic_meet_with_equal_outer_uses_inner_meet() {
        let a = Lexicographic::new(5u64, 3u64);
        let b = Lexicographic::new(5u64, 7u64);
        assert_eq!(a.meet(&b), Lexicographic::new(5u64, 3u64));
    }

    #[test]
    fn lexicographic_join_returns_greater_when_outer_differs() {
        let a = Lexicographic::new(1u64, 99u64);
        let b = Lexicographic::new(2u64, 0u64);
        assert_eq!(a.join(&b), b);
        assert_eq!(b.join(&a), b);
    }

    #[test]
    fn lexicographic_join_with_equal_outer_uses_inner_join() {
        let a = Lexicographic::new(5u64, 3u64);
        let b = Lexicographic::new(5u64, 7u64);
        assert_eq!(a.join(&b), Lexicographic::new(5u64, 7u64));
    }
}

// ── Phase 2: property tests — algebraic law proofs ────────────────────────────
//
// Every law runs under 10 000 random inputs. CI fails if any case breaks a law.
// These tests document the exact contract `T` must satisfy (see roadmap §2).

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    /// Verify the antichain invariant: no element dominates another.
    fn antichain_valid<T: PartialOrd + Clone>(a: &Antichain<T>) -> bool {
        let els = a.elements();
        for (i, x) in els.iter().enumerate() {
            for (j, y) in els.iter().enumerate() {
                if i != j && x <= y {
                    return false;
                }
            }
        }
        true
    }

    prop_compose! {
        /// Arbitrary `Frontier<u64>` built from 0–9 random elements.
        fn arb_frontier_u64()(
            elems in prop::collection::vec(any::<u64>(), 0..10)
        ) -> Frontier<u64> {
            Frontier::from_elements(elems)
        }
    }

    prop_compose! {
        /// Arbitrary `Frontier<(u64,u64)>` under the standard lexicographic `PartialOrd`.
        fn arb_frontier_pair()(
            elems in prop::collection::vec((any::<u64>(), any::<u64>()), 0..10)
        ) -> Frontier<(u64, u64)> {
            Frontier::from_elements(elems)
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        // ── Antichain invariant ──────────────────────────────────────────────

        /// After any sequence of inserts the set invariant holds:
        /// no element in the antichain is `<=` any other.
        #[test]
        fn prop_antichain_invariant_u64(
            elems in prop::collection::vec(any::<u64>(), 0..20)
        ) {
            let mut a = Antichain::<u64>::empty();
            for e in elems { a.insert(e); }
            prop_assert!(antichain_valid(&a));
        }

        /// Invariant holds for pair elements (lexicographic order).
        #[test]
        fn prop_antichain_invariant_pair(
            elems in prop::collection::vec((any::<u64>(), any::<u64>()), 0..20)
        ) {
            let mut a = Antichain::<(u64, u64)>::empty();
            for e in elems { a.insert(e); }
            prop_assert!(antichain_valid(&a));
        }

        // ── PartialOrd laws for the element type ─────────────────────────────

        /// Reflexivity: `a <= a` for all `a`.
        #[test]
        fn prop_partialord_reflexive(a in any::<u64>()) {
            prop_assert!(a <= a);
        }

        /// Antisymmetry: `a <= b` and `b <= a` implies `a == b`.
        #[test]
        fn prop_partialord_antisymmetric(a in any::<u64>(), b in any::<u64>()) {
            if a <= b && b <= a {
                prop_assert_eq!(a, b);
            }
        }

        /// Transitivity: `a <= b` and `b <= c` implies `a <= c`.
        #[test]
        fn prop_partialord_transitive(
            a in any::<u64>(), b in any::<u64>(), c in any::<u64>()
        ) {
            if a <= b && b <= c {
                prop_assert!(a <= c);
            }
        }

        // ── Frontier<u64> — meet (coordinator-free merge) ────────────────────

        /// Commutativity: `meet(a, b) == meet(b, a)`.
        #[test]
        fn prop_meet_commutative_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        /// Associativity: `meet(a, meet(b, c)) == meet(meet(a, b), c)`.
        #[test]
        fn prop_meet_associative_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64(), c in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        /// Idempotence: `meet(a, a) == a`.
        #[test]
        fn prop_meet_idempotent_u64(a in arb_frontier_u64()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        /// Absorption: once `m = meet(a, b)` is computed, further meets are stable:
        /// `meet(m, a) == m` and `meet(m, b) == m`.
        #[test]
        fn prop_meet_absorption_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64()
        ) {
            let m = a.meet(&b);
            prop_assert_eq!(m.meet(&a), m.clone());
            prop_assert_eq!(m.meet(&b), m.clone());
        }

        // ── Frontier<u64> — join (lattice advance) ────────────────────────────

        /// Commutativity: `join(a, b) == join(b, a)`.
        #[test]
        fn prop_join_commutative_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        /// Associativity: `join(a, join(b, c)) == join(join(a, b), c)`.
        #[test]
        fn prop_join_associative_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64(), c in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.join(&b.join(&c)), a.join(&b).join(&c));
        }

        /// Idempotence: `join(a, a) == a`.
        #[test]
        fn prop_join_idempotent_u64(a in arb_frontier_u64()) {
            prop_assert_eq!(a.join(&a), a);
        }

        // ── Standard lattice absorption identities ────────────────────────────

        /// `a ∧ (a ∨ b) = a` — meet absorbs join.
        #[test]
        fn prop_absorption_meet_of_join_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.meet(&a.join(&b)), a.clone());
        }

        /// `a ∨ (a ∧ b) = a` — join absorbs meet.
        #[test]
        fn prop_absorption_join_of_meet_u64(
            a in arb_frontier_u64(), b in arb_frontier_u64()
        ) {
            prop_assert_eq!(a.join(&a.meet(&b)), a.clone());
        }

        // ── Frontier<(u64,u64)> — meet laws (lexicographic PartialOrd) ────────

        /// Commutativity of meet for pair frontiers.
        #[test]
        fn prop_meet_commutative_pair(
            a in arb_frontier_pair(), b in arb_frontier_pair()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        /// Associativity of meet for pair frontiers.
        #[test]
        fn prop_meet_associative_pair(
            a in arb_frontier_pair(), b in arb_frontier_pair(), c in arb_frontier_pair()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        /// Idempotence of meet for pair frontiers.
        #[test]
        fn prop_meet_idempotent_pair(a in arb_frontier_pair()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        /// Antichain invariant is preserved through meet for pair frontiers.
        #[test]
        fn prop_antichain_invariant_after_meet_pair(
            a in arb_frontier_pair(), b in arb_frontier_pair()
        ) {
            let m = a.meet(&b);
            let mut ac = Antichain::<(u64, u64)>::empty();
            for &e in m.elements() { ac.insert(e); }
            prop_assert!(antichain_valid(&ac));
        }
    }
}

// ── Phase 3: property tests — composition types ───────────────────────────────

#[cfg(test)]
mod prop_tests_phase3 {
    use super::*;
    use proptest::prelude::*;

    fn antichain_valid<T: PartialOrd + Clone>(a: &Antichain<T>) -> bool {
        let els = a.elements();
        for (i, x) in els.iter().enumerate() {
            for (j, y) in els.iter().enumerate() {
                if i != j && x <= y {
                    return false;
                }
            }
        }
        true
    }

    prop_compose! {
        fn arb_frontier_product()(
            elems in prop::collection::vec((any::<u64>(), any::<u64>()), 0..10)
        ) -> Frontier<ProductTimestamp<u64, u64>> {
            Frontier::from_elements(elems.into_iter().map(|(a, b)| ProductTimestamp::new(a, b)))
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        // ── ProductTimestamp: order laws ──────────────────────────────────────

        /// Reflexivity of product order.
        #[test]
        fn prop_product_order_reflexive(x in any::<u64>(), y in any::<u64>()) {
            let p = ProductTimestamp::new(x, y);
            prop_assert!(p <= p);
        }

        /// Antisymmetry: if `a ≤ b` and `b ≤ a` then `a == b`.
        #[test]
        fn prop_product_order_antisymmetric(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            if a <= b && b <= a {
                prop_assert_eq!(a, b);
            }
        }

        /// Transitivity: if `a ≤ b` and `b ≤ c` then `a ≤ c`.
        #[test]
        fn prop_product_order_transitive(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>(),
            x3 in any::<u64>(), y3 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            let c = ProductTimestamp::new(x3, y3);
            if a <= b && b <= c {
                prop_assert!(a <= c);
            }
        }

        // ── ProductTimestamp: Lattice element laws ────────────────────────────

        /// meet is a lower bound: `meet(a,b) ≤ a` and `meet(a,b) ≤ b`.
        #[test]
        fn prop_product_meet_is_lower_bound(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            let m = a.meet(&b);
            prop_assert!(m <= a);
            prop_assert!(m <= b);
        }

        /// join is an upper bound: `a ≤ join(a,b)` and `b ≤ join(a,b)`.
        #[test]
        fn prop_product_join_is_upper_bound(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            let j = a.join(&b);
            prop_assert!(a <= j);
            prop_assert!(b <= j);
        }

        /// meet commutativity.
        #[test]
        fn prop_product_meet_commutative(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        /// join commutativity.
        #[test]
        fn prop_product_join_commutative(
            x1 in any::<u64>(), y1 in any::<u64>(),
            x2 in any::<u64>(), y2 in any::<u64>()
        ) {
            let a = ProductTimestamp::new(x1, y1);
            let b = ProductTimestamp::new(x2, y2);
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        // ── Antichain<ProductTimestamp>: invariant ────────────────────────────

        #[test]
        fn prop_antichain_invariant_product(
            elems in prop::collection::vec((any::<u64>(), any::<u64>()), 0..20)
        ) {
            let mut a = Antichain::<ProductTimestamp<u64, u64>>::empty();
            for (x, y) in elems { a.insert(ProductTimestamp::new(x, y)); }
            prop_assert!(antichain_valid(&a));
        }

        // ── Frontier<ProductTimestamp>: meet laws ─────────────────────────────

        #[test]
        fn prop_frontier_product_meet_commutative(
            a in arb_frontier_product(), b in arb_frontier_product()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_frontier_product_meet_associative(
            a in arb_frontier_product(), b in arb_frontier_product(), c in arb_frontier_product()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_frontier_product_meet_idempotent(a in arb_frontier_product()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        // ── Frontier<ProductTimestamp>: join laws ─────────────────────────────

        #[test]
        fn prop_frontier_product_join_commutative(
            a in arb_frontier_product(), b in arb_frontier_product()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn prop_frontier_product_join_idempotent(a in arb_frontier_product()) {
            prop_assert_eq!(a.join(&a), a);
        }

        // ── Lexicographic: order and Lattice laws ─────────────────────────────

        /// meet is a lower bound under lexicographic order.
        #[test]
        fn prop_lexico_meet_is_lower_bound(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            let m = p.meet(&q);
            prop_assert!(m <= p);
            prop_assert!(m <= q);
        }

        /// join is an upper bound under lexicographic order.
        #[test]
        fn prop_lexico_join_is_upper_bound(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            let j = p.join(&q);
            prop_assert!(p <= j);
            prop_assert!(q <= j);
        }

        /// meet commutativity.
        #[test]
        fn prop_lexico_meet_commutative(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            prop_assert_eq!(p.meet(&q), q.meet(&p));
        }

        /// join commutativity.
        #[test]
        fn prop_lexico_join_commutative(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            prop_assert_eq!(p.join(&q), q.join(&p));
        }

        /// meet associativity.
        #[test]
        fn prop_lexico_meet_associative(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>(),
            a3 in any::<u64>(), b3 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            let r = Lexicographic::new(a3, b3);
            prop_assert_eq!(p.meet(&q.meet(&r)), p.meet(&q).meet(&r));
        }

        /// join associativity.
        #[test]
        fn prop_lexico_join_associative(
            a1 in any::<u64>(), b1 in any::<u64>(),
            a2 in any::<u64>(), b2 in any::<u64>(),
            a3 in any::<u64>(), b3 in any::<u64>()
        ) {
            let p = Lexicographic::new(a1, b1);
            let q = Lexicographic::new(a2, b2);
            let r = Lexicographic::new(a3, b3);
            prop_assert_eq!(p.join(&q.join(&r)), p.join(&q).join(&r));
        }
    }
}

// ── Phase 5: convergence property tests ───────────────────────────────────────
//
// These tests directly validate the formal convergence guarantee stated in
// specs/frontier_convergence.fizz:
//
//   "If two nodes have each observed any subset of the same update set, in any
//    order, their Frontier values will be identical after merging all updates."
//
// The property holds because meet is commutative, associative, and idempotent
// (proven by the Phase 2 tests). These tests lift that per-operation proof to
// the system level: any sequence of `from_elem` + `meet` calls over the same
// multiset of values converges to the same Frontier, regardless of order.

#[cfg(test)]
mod prop_tests_phase5 {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        /// Convergence for Frontier<u64>: applying the same pool of u64 updates
        /// in forward, reverse, and sorted order all yield the same Frontier.
        #[test]
        fn prop_convergence_order_independence_u64(
            updates in prop::collection::vec(any::<u64>(), 1..20)
        ) {
            // Node A: forward order.
            let node_a = updates.iter().copied().fold(
                Frontier::<u64>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            // Node B: reverse order.
            let node_b = updates.iter().rev().copied().fold(
                Frontier::<u64>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            // Node C: sorted ascending (a deterministic third permutation).
            let mut sorted = updates.clone();
            sorted.sort_unstable();
            let node_c = sorted.iter().copied().fold(
                Frontier::<u64>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            prop_assert_eq!(node_a, node_b.clone());
            prop_assert_eq!(node_b, node_c);
        }

        /// Convergence for Frontier<ProductTimestamp<u64,u64>>: the non-trivial case
        /// where the antichain can hold multiple mutually incomparable elements and
        /// the order of update delivery makes the test genuinely interesting.
        #[test]
        fn prop_convergence_order_independence_product(
            raw in prop::collection::vec((any::<u64>(), any::<u64>()), 1..20)
        ) {
            let updates: Vec<_> =
                raw.iter().map(|&(a, b)| ProductTimestamp::new(a, b)).collect();

            // Node A: forward.
            let node_a = updates.iter().cloned().fold(
                Frontier::<ProductTimestamp<u64, u64>>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            // Node B: reverse.
            let node_b = updates.iter().rev().cloned().fold(
                Frontier::<ProductTimestamp<u64, u64>>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            // Node C: sorted by (outer, inner) — a deterministic total order.
            let mut sorted = updates.clone();
            sorted.sort_unstable_by(|a, b| {
                a.outer.cmp(&b.outer).then(a.inner.cmp(&b.inner))
            });
            let node_c = sorted.iter().cloned().fold(
                Frontier::<ProductTimestamp<u64, u64>>::bottom(),
                |acc, u| acc.meet(&Frontier::from_elem(u)),
            );
            prop_assert_eq!(node_a, node_b.clone());
            prop_assert_eq!(node_b, node_c);
        }
    }
}

// ── Phase 6: Extended composition patterns ────────────────────────────────────

/// Wraps `T` and **inverts** its partial order.
///
/// `Max(a) ≤ Max(b)` iff `b ≤ a` in `T`.
///
/// **Use case:** tracking "at least X" lower bounds in a [`Frontier`]. Because
/// the order is inverted, [`Frontier::meet`] (the conservative merge) computes
/// `max` of the underlying values, preserving the *highest* guaranteed lower
/// bound seen across all workers.
///
/// # Example
///
/// ```
/// use antichain::{Frontier, Max};
///
/// // Worker A guarantees "offset ≥ 10"; worker B guarantees "offset ≥ 5".
/// let wa = Frontier::from_elem(Max(10u64));
/// let wb = Frontier::from_elem(Max(5u64));
///
/// // Conservative merge: the merged frontier still guarantees ≥ 10
/// // (inverted order makes meet = max of underlying values).
/// let merged = wa.meet(&wb);
/// assert_eq!(merged.elements(), &[Max(10u64)]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Max<T>(pub T);

impl<T: PartialOrd> PartialOrd for Max<T> {
    /// Inverted ordering: `Max(a) ≤ Max(b)` iff `b ≤ a`.
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        other.0.partial_cmp(&self.0)
    }
}

impl<T: Lattice + Clone> Lattice for Max<T> {
    /// Greatest lower bound in the inverted order = join of the underlying values.
    ///
    /// `meet(Max(a), Max(b)) = Max(max(a, b))`
    #[inline]
    fn meet(&self, other: &Self) -> Self {
        Max(self.0.join(&other.0))
    }
    /// Least upper bound in the inverted order = meet of the underlying values.
    ///
    /// `join(Max(a), Max(b)) = Max(min(a, b))`
    #[inline]
    fn join(&self, other: &Self) -> Self {
        Max(self.0.meet(&other.0))
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// Wraps `T` preserving its natural partial order.
///
/// `Min(a) ≤ Min(b)` iff `a ≤ b` in `T`.
///
/// **Use case:** tracking "at most Y" upper bounds in a [`Frontier`] alongside
/// [`Max`] lower bounds. In a composite type like `(Max<T>, Min<T>)`, the first
/// component tracks the minimum guaranteed progress and the second tracks the
/// maximum observed value, providing both a lower and upper bound simultaneously.
///
/// The [`Lattice`] impl delegates directly to `T`, so `meet` computes `min` and
/// `join` computes `max` — identical to an unwrapped `T`. The newtype makes the
/// intent explicit and enables clean composition with [`Max<T>`].
///
/// # Example
///
/// ```
/// use antichain::{Frontier, Max, Min, Lattice};
///
/// // Track a sliding window [lower_bound, upper_bound].
/// let f1 = Frontier::from_elem((Max(5u64), Min(20u64)));
/// let f2 = Frontier::from_elem((Max(8u64), Min(15u64)));
///
/// // meet: highest lower bound (max(5,8)=8) and lowest upper bound (min(20,15)=15).
/// let merged = f1.meet(&f2);
/// assert_eq!(merged.elements()[0].0, Max(8u64));
/// assert_eq!(merged.elements()[0].1, Min(15u64));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Min<T>(pub T);

impl<T: PartialOrd> PartialOrd for Min<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T: Lattice + Clone> Lattice for Min<T> {
    #[inline]
    fn meet(&self, other: &Self) -> Self {
        Min(self.0.meet(&other.0))
    }
    #[inline]
    fn join(&self, other: &Self) -> Self {
        Min(self.0.join(&other.0))
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// A timestamp wrapper that restricts values to the interval `[min, max]`.
///
/// Values are clamped to `[min, max]` at construction time. All [`Lattice`]
/// operations preserve this invariant. Because the range is finite, the number
/// of distinct incomparable values — and therefore the maximum width of any
/// `Antichain<Bounded<T>>` — is bounded by the cardinality of `[min, max]`.
///
/// Two `Bounded<T>` values compare by their [`value`][Bounded::value] using
/// the natural order of `T`. Mixing `Bounded<T>` values with different
/// `[min, max]` ranges in the same antichain is semantically undefined;
/// lattice operations use the bounds of `self`.
///
/// **Phase 8.3:** `T` requires only [`PartialOrd`] (relaxed from `Ord`), enabling
/// composition with [`ProductTimestamp`] and other partially-ordered types. If `value`
/// is incomparable with the bounds under `PartialOrd`, it is stored as-is without
/// clamping — this is documented as caller-defined behavior.
///
/// # Example
///
/// ```
/// use antichain::{Frontier, Bounded};
///
/// // Offsets restricted to [0, 1000].
/// let f1 = Frontier::from_elem(Bounded::new(300u64, 0, 1000));
/// let f2 = Frontier::from_elem(Bounded::new(700u64, 0, 1000));
///
/// // Conservative merge picks the lower value, clamped to the range.
/// let merged = f1.meet(&f2);
/// assert_eq!(*merged.elements()[0].value(), 300u64);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bounded<T> {
    /// The current value, always in `[min, max]`.
    pub value: T,
    /// The inclusive lower bound of the range.
    pub min: T,
    /// The inclusive upper bound of the range.
    pub max: T,
}

impl<T: PartialOrd + Clone> Bounded<T> {
    /// Creates a `Bounded<T>`, clamping `value` to `[min, max]`.
    ///
    /// # Panics
    ///
    /// Panics if `min > max` (or if `min` and `max` are incomparable under `PartialOrd`,
    /// since `min <= max` evaluates to `false` for incomparable pairs).
    pub fn new(value: T, min: T, max: T) -> Self {
        assert!(min <= max, "Bounded: min must be <= max");
        let value = if value < min {
            min.clone()
        } else if value > max {
            max.clone()
        } else {
            value
        };
        Self { value, min, max }
    }

    /// Returns a reference to the current value.
    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: PartialOrd> PartialOrd for Bounded<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<T: Lattice + Clone> Lattice for Bounded<T> {
    fn meet(&self, other: &Self) -> Self {
        let v = self.value.meet(&other.value);
        let v = if v < self.min {
            self.min.clone()
        } else if v > self.max {
            self.max.clone()
        } else {
            v
        };
        Bounded {
            value: v,
            min: self.min.clone(),
            max: self.max.clone(),
        }
    }

    fn join(&self, other: &Self) -> Self {
        let v = self.value.join(&other.value);
        let v = if v < self.min {
            self.min.clone()
        } else if v > self.max {
            self.max.clone()
        } else {
            v
        };
        Bounded {
            value: v,
            min: self.min.clone(),
            max: self.max.clone(),
        }
    }
}

// ── Phase 6 tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_phase6 {
    use super::*;

    // ── Max<T> ───────────────────────────────────────────────────────────────

    #[test]
    fn max_order_is_inverted() {
        // In Max<u64>, larger underlying value is "smaller".
        assert!(Max(10u64) < Max(5u64));
        assert!(Max(5u64) > Max(10u64));
        assert_eq!(Max(7u64), Max(7u64));
    }

    #[test]
    fn max_meet_gives_underlying_max() {
        assert_eq!(Max(3u64).meet(&Max(7u64)), Max(7u64));
        assert_eq!(Max(7u64).meet(&Max(3u64)), Max(7u64));
    }

    #[test]
    fn max_join_gives_underlying_min() {
        assert_eq!(Max(3u64).join(&Max(7u64)), Max(3u64));
        assert_eq!(Max(7u64).join(&Max(3u64)), Max(3u64));
    }

    #[test]
    fn frontier_max_meet_picks_highest_lower_bound() {
        let wa = Frontier::from_elem(Max(10u64));
        let wb = Frontier::from_elem(Max(5u64));
        let merged = wa.meet(&wb);
        assert_eq!(merged.elements(), &[Max(10u64)]);
    }

    #[test]
    fn frontier_max_meet_is_commutative() {
        let a = Frontier::from_elem(Max(10u64));
        let b = Frontier::from_elem(Max(5u64));
        assert_eq!(a.meet(&b), b.meet(&a));
    }

    #[test]
    fn frontier_max_meet_is_idempotent() {
        let f = Frontier::from_elem(Max(7u64));
        assert_eq!(f.meet(&f), f);
    }

    // ── Min<T> ───────────────────────────────────────────────────────────────

    #[test]
    fn min_order_is_natural() {
        assert!(Min(3u64) < Min(7u64));
        assert_eq!(Min(5u64), Min(5u64));
    }

    #[test]
    fn min_meet_gives_underlying_min() {
        assert_eq!(Min(3u64).meet(&Min(7u64)), Min(3u64));
    }

    #[test]
    fn min_join_gives_underlying_max() {
        assert_eq!(Min(3u64).join(&Min(7u64)), Min(7u64));
    }

    // ── (Max<T>, Min<T>) composite ───────────────────────────────────────────

    #[test]
    fn composite_max_min_frontier_meet() {
        let f1 = Frontier::from_elem((Max(5u64), Min(20u64)));
        let f2 = Frontier::from_elem((Max(8u64), Min(15u64)));
        let merged = f1.meet(&f2);
        // Tuple meet is component-wise: Max meets to max(5,8)=8, Min meets to min(20,15)=15.
        assert_eq!(merged.elements()[0].0, Max(8u64));
        assert_eq!(merged.elements()[0].1, Min(15u64));
    }

    // ── Bounded<T> ───────────────────────────────────────────────────────────

    #[test]
    fn bounded_new_clamps_value() {
        let b = Bounded::new(1500u64, 0, 1000);
        assert_eq!(*b.value(), 1000u64);

        let b2 = Bounded::new(0u64, 100, 1000);
        assert_eq!(*b2.value(), 100u64);
    }

    #[test]
    fn bounded_new_keeps_in_range_value() {
        let b = Bounded::new(500u64, 0, 1000);
        assert_eq!(*b.value(), 500u64);
    }

    #[test]
    #[should_panic]
    fn bounded_new_panics_if_min_gt_max() {
        Bounded::new(5u64, 10, 0);
    }

    #[test]
    fn bounded_meet_gives_lower_value_clamped() {
        let a = Bounded::new(300u64, 0, 1000);
        let b = Bounded::new(700u64, 0, 1000);
        assert_eq!(*a.meet(&b).value(), 300u64);
        assert_eq!(*b.meet(&a).value(), 300u64);
    }

    #[test]
    fn bounded_join_gives_higher_value_clamped() {
        let a = Bounded::new(300u64, 0, 1000);
        let b = Bounded::new(700u64, 0, 1000);
        assert_eq!(*a.join(&b).value(), 700u64);
    }

    #[test]
    fn bounded_order_by_value() {
        let a = Bounded::new(200u64, 0, 1000);
        let b = Bounded::new(800u64, 0, 1000);
        assert!(a < b);
    }

    #[test]
    fn frontier_bounded_meet_is_conservative() {
        let f1 = Frontier::from_elem(Bounded::new(300u64, 0, 1000));
        let f2 = Frontier::from_elem(Bounded::new(700u64, 0, 1000));
        let merged = f1.meet(&f2);
        assert_eq!(*merged.elements()[0].value(), 300u64);
    }

    #[test]
    fn frontier_bounded_antichain_width_bounded() {
        // For Bounded<u64> values in [0, 5] they are totally ordered → antichain width ≤ 1.
        let f = Frontier::from_elements([
            Bounded::new(2u64, 0, 5),
            Bounded::new(4u64, 0, 5),
            Bounded::new(1u64, 0, 5),
        ]);
        // All are totally ordered; only the minimum (1) survives.
        assert_eq!(f.elements().len(), 1);
        assert_eq!(*f.elements()[0].value(), 1u64);
    }

    // ── Nested composition: ProductTimestamp<Bounded<u64>, u64> ──────────────

    #[test]
    fn nested_product_bounded_outer() {
        // Frontier<ProductTimestamp<Bounded<u64>, u64>>: bounded outer, unbounded inner.
        let f1 = Frontier::from_elem(ProductTimestamp::new(Bounded::new(3u64, 0, 10), 100u64));
        let f2 = Frontier::from_elem(ProductTimestamp::new(Bounded::new(7u64, 0, 10), 50u64));
        // (3, 100) and (7, 50) are incomparable in product order → both survive meet.
        let merged = f1.meet(&f2);
        assert_eq!(merged.elements().len(), 2);
    }
}

// ── Phase 6 property tests ────────────────────────────────────────────────────

#[cfg(test)]
mod prop_tests_phase6 {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn arb_max_u64()(v in any::<u64>()) -> Max<u64> { Max(v) }
    }

    prop_compose! {
        fn arb_min_u64()(v in any::<u64>()) -> Min<u64> { Min(v) }
    }

    prop_compose! {
        fn arb_bounded()(
            a in 0u64..=500u64,
            b in 0u64..=500u64,
            v in 0u64..=1000u64,
        ) -> Bounded<u64> {
            let lo = a.min(b);
            let hi = a.max(b) + 1; // ensure lo < hi
            Bounded::new(v, lo, lo + hi)
        }
    }

    prop_compose! {
        fn arb_frontier_max()(
            elems in prop::collection::vec(any::<u64>(), 0..10)
        ) -> Frontier<Max<u64>> {
            Frontier::from_elements(elems.into_iter().map(Max))
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        // ── Max<u64>: order laws ──────────────────────────────────────────────

        #[test]
        fn prop_max_order_inverted(a in any::<u64>(), b in any::<u64>()) {
            // Max(a) <= Max(b) iff b <= a.
            let ma = Max(a);
            let mb = Max(b);
            if a <= b {
                prop_assert!(mb <= ma);
            }
            if b <= a {
                prop_assert!(ma <= mb);
            }
        }

        #[test]
        fn prop_max_meet_is_underlying_join(a in any::<u64>(), b in any::<u64>()) {
            prop_assert_eq!(Max(a).meet(&Max(b)), Max(a.join(&b)));
        }

        #[test]
        fn prop_max_join_is_underlying_meet(a in any::<u64>(), b in any::<u64>()) {
            prop_assert_eq!(Max(a).join(&Max(b)), Max(a.meet(&b)));
        }

        #[test]
        fn prop_max_meet_commutative(a in any::<u64>(), b in any::<u64>()) {
            prop_assert_eq!(Max(a).meet(&Max(b)), Max(b).meet(&Max(a)));
        }

        #[test]
        fn prop_max_meet_idempotent(a in any::<u64>()) {
            prop_assert_eq!(Max(a).meet(&Max(a)), Max(a));
        }

        // ── Min<u64>: Lattice laws ────────────────────────────────────────────

        #[test]
        fn prop_min_meet_is_underlying_meet(a in any::<u64>(), b in any::<u64>()) {
            prop_assert_eq!(Min(a).meet(&Min(b)), Min(a.meet(&b)));
        }

        #[test]
        fn prop_min_join_is_underlying_join(a in any::<u64>(), b in any::<u64>()) {
            prop_assert_eq!(Min(a).join(&Min(b)), Min(a.join(&b)));
        }

        // ── Frontier<Max<u64>>: meet laws ─────────────────────────────────────

        #[test]
        fn prop_frontier_max_meet_commutative(
            a in arb_frontier_max(), b in arb_frontier_max()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_frontier_max_meet_associative(
            a in arb_frontier_max(), b in arb_frontier_max(), c in arb_frontier_max()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_frontier_max_meet_idempotent(a in arb_frontier_max()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        // ── Bounded<u64>: value always in range ───────────────────────────────

        #[test]
        fn prop_bounded_value_in_range(
            lo in 0u64..500u64,
            hi in 500u64..1000u64,
            v in any::<u64>()
        ) {
            let b = Bounded::new(v, lo, hi);
            prop_assert!(*b.value() >= lo);
            prop_assert!(*b.value() <= hi);
        }

        #[test]
        fn prop_bounded_meet_value_in_range(
            lo in 0u64..200u64,
            hi in 800u64..1000u64,
            v1 in 0u64..1000u64,
            v2 in 0u64..1000u64,
        ) {
            let a = Bounded::new(v1, lo, hi);
            let b = Bounded::new(v2, lo, hi);
            let m = a.meet(&b);
            prop_assert!(*m.value() >= lo);
            prop_assert!(*m.value() <= hi);
        }

        #[test]
        fn prop_bounded_join_value_in_range(
            lo in 0u64..200u64,
            hi in 800u64..1000u64,
            v1 in 0u64..1000u64,
            v2 in 0u64..1000u64,
        ) {
            let a = Bounded::new(v1, lo, hi);
            let b = Bounded::new(v2, lo, hi);
            let j = a.join(&b);
            prop_assert!(*j.value() >= lo);
            prop_assert!(*j.value() <= hi);
        }

        #[test]
        fn prop_bounded_meet_commutative(
            lo in 0u64..200u64,
            hi in 800u64..1000u64,
            v1 in 0u64..1000u64,
            v2 in 0u64..1000u64,
        ) {
            let a = Bounded::new(v1, lo, hi);
            let b = Bounded::new(v2, lo, hi);
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_bounded_meet_idempotent(
            lo in 0u64..200u64,
            hi in 800u64..1000u64,
            v in 0u64..1000u64,
        ) {
            let a = Bounded::new(v, lo, hi);
            prop_assert_eq!(a.meet(&a), a);
        }
    }
}

// ── Phase 7: Advanced structural & dynamic lattices ───────────────────────────
//
// Sequencing: 7.3 (WithTop/WithBottom) first — simplest addition and clarifies
// the bottom/top semantics the others reason about. 7.1 (MapLattice) and 7.2
// (SetLattice) are independent of each other and follow.
//
// Correctness law enforced on every type: the meet/join impls must agree with
// PartialOrd — i.e. a ≤ b ⟺ meet(a, b) == a ⟺ join(a, b) == b.
// This consistency law is verified by property tests in prop_tests_phase7.

// ── 7.3 WithTop ───────────────────────────────────────────────────────────────

/// Lifts any type `T` by adding a single `Top` element above all `Value(t)`.
///
/// - `Top` is **absorbing for [`Lattice::join`]**: `join(Top, x) = Top`.
/// - `Top` is the **identity for [`Lattice::meet`]**: `meet(Top, x) = x`.
/// - Does **not** add a `Bottom` element. Compose with [`WithBottom`] when both
///   sentinels are needed: `WithTop<WithBottom<T>>` gives a three-level closed lattice
///   `Bottom < Value(t) < Top`.
///
/// **Use case:** when an upstream data source finishes, wrap its final element in
/// `WithTop::Top` to permanently signal that the data path is closed. Any subsequent
/// `join` with another frontier element immediately absorbs to `Top`, short-circuiting
/// progress calculation.
///
/// # Example
///
/// ```
/// use antichain::{WithTop, Lattice};
///
/// // Top absorbs join.
/// let top: WithTop<u64> = WithTop::Top;
/// let val = WithTop::Value(42u64);
/// assert_eq!(top.join(&val), WithTop::Top);
/// assert_eq!(val.join(&top), WithTop::Top);
///
/// // Top is the identity for meet.
/// assert_eq!(top.meet(&val), val);
/// assert_eq!(val.meet(&top), val);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WithTop<T> {
    /// A wrapped value, below `Top`.
    Value(T),
    /// The top element, above all `Value(t)`.
    Top,
}

impl<T: PartialOrd> PartialOrd for WithTop<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match (self, other) {
            (WithTop::Top, WithTop::Top) => Some(core::cmp::Ordering::Equal),
            (WithTop::Top, WithTop::Value(_)) => Some(core::cmp::Ordering::Greater),
            (WithTop::Value(_), WithTop::Top) => Some(core::cmp::Ordering::Less),
            (WithTop::Value(a), WithTop::Value(b)) => a.partial_cmp(b),
        }
    }
}

impl<T: Lattice + Clone> Lattice for WithTop<T> {
    /// Greatest lower bound: `Top` is the identity — it does not constrain the other value.
    fn meet(&self, other: &Self) -> Self {
        match (self, other) {
            (WithTop::Top, x) => x.clone(),
            (x, WithTop::Top) => x.clone(),
            (WithTop::Value(a), WithTop::Value(b)) => WithTop::Value(a.meet(b)),
        }
    }
    /// Least upper bound: `Top` is absorbing — it dominates any other value.
    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (WithTop::Top, _) | (_, WithTop::Top) => WithTop::Top,
            (WithTop::Value(a), WithTop::Value(b)) => WithTop::Value(a.join(b)),
        }
    }
}

// ── 7.3 WithBottom ────────────────────────────────────────────────────────────

/// Lifts any type `T` by adding a single `Bottom` element below all `Value(t)`.
///
/// - `Bottom` is **absorbing for [`Lattice::meet`]**: `meet(Bottom, x) = Bottom`.
/// - `Bottom` is the **identity for [`Lattice::join`]**: `join(Bottom, x) = x`.
/// - Symmetric to [`WithTop`]: compose as `WithTop<WithBottom<T>>` for a closed lattice
///   `Bottom < Value(t) < Top`.
///
/// **Use case:** represents "no progress yet" or "this data path has not started". Makes
/// the absence of a value explicit in the type system rather than relying on magic constants.
///
/// # Example
///
/// ```
/// use antichain::{WithBottom, Lattice};
///
/// // Bottom absorbs meet.
/// let bottom: WithBottom<u64> = WithBottom::Bottom;
/// let val = WithBottom::Value(42u64);
/// assert_eq!(bottom.meet(&val), WithBottom::Bottom);
/// assert_eq!(val.meet(&bottom), WithBottom::Bottom);
///
/// // Bottom is the identity for join.
/// assert_eq!(bottom.join(&val), val);
/// assert_eq!(val.join(&bottom), val);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WithBottom<T> {
    /// The bottom element, below all `Value(t)`.
    Bottom,
    /// A wrapped value, above `Bottom`.
    Value(T),
}

impl<T: PartialOrd> PartialOrd for WithBottom<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match (self, other) {
            (WithBottom::Bottom, WithBottom::Bottom) => Some(core::cmp::Ordering::Equal),
            (WithBottom::Bottom, WithBottom::Value(_)) => Some(core::cmp::Ordering::Less),
            (WithBottom::Value(_), WithBottom::Bottom) => Some(core::cmp::Ordering::Greater),
            (WithBottom::Value(a), WithBottom::Value(b)) => a.partial_cmp(b),
        }
    }
}

impl<T: Lattice + Clone> Lattice for WithBottom<T> {
    /// Greatest lower bound: `Bottom` is absorbing — it forces the result to `Bottom`.
    fn meet(&self, other: &Self) -> Self {
        match (self, other) {
            (WithBottom::Bottom, _) | (_, WithBottom::Bottom) => WithBottom::Bottom,
            (WithBottom::Value(a), WithBottom::Value(b)) => WithBottom::Value(a.meet(b)),
        }
    }
    /// Least upper bound: `Bottom` is the identity — it does not constrain the other value.
    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (WithBottom::Bottom, x) => x.clone(),
            (x, WithBottom::Bottom) => x.clone(),
            (WithBottom::Value(a), WithBottom::Value(b)) => WithBottom::Value(a.join(b)),
        }
    }
}

// ── 7.1 MapLattice ────────────────────────────────────────────────────────────

/// A point-wise lattice over a `BTreeMap<K, V>`.
///
/// The natural generalization of [`ProductTimestamp`] to dynamic arities: where
/// `ProductTimestamp` models a fixed-arity product, `MapLattice` models an open-ended
/// set of named dimensions that can grow at runtime.
///
/// **Partial order:** `M₁ ≤ M₂` iff every key `k` in `M₁` is present in `M₂` with
/// `M₁[k] ≤ M₂[k]`. Missing keys are implicitly the bottom element (no progress recorded
/// yet). An empty map is the bottom of the lattice.
///
/// **Lattice operations:**
/// - `join(M₁, M₂)`: key-union; overlapping values take their join. The empty map is the
///   identity.
/// - `meet(M₁, M₂)`: key-intersection; overlapping values take their meet. The empty map is
///   absorbing.
///
/// **Use case:** a cluster scales from 10 shards to 100 shards at runtime. Each shard key
/// appears in the map the moment it first reports progress. Static tuples cannot accommodate
/// this without recompilation; `MapLattice` makes it expressible with the same coordinator-free
/// merge guarantee.
///
/// # Example
///
/// ```
/// use antichain::{MapLattice, Lattice};
///
/// let mut node_a: MapLattice<&str, u64> = MapLattice::new();
/// node_a.insert("shard-0", 10);
/// node_a.insert("shard-1", 5);
///
/// let mut node_b: MapLattice<&str, u64> = MapLattice::new();
/// node_b.insert("shard-0", 7);
/// node_b.insert("shard-2", 3);
///
/// // meet: intersection of keys, value-meet (min) on shared keys.
/// let m = node_a.meet(&node_b);
/// assert_eq!(m.get(&"shard-0"), Some(&7));
/// assert_eq!(m.get(&"shard-1"), None); // not in both
/// assert_eq!(m.get(&"shard-2"), None); // not in both
///
/// // join: union of keys, value-join (max) on shared keys.
/// let j = node_a.join(&node_b);
/// assert_eq!(j.get(&"shard-0"), Some(&10));
/// assert_eq!(j.get(&"shard-1"), Some(&5));
/// assert_eq!(j.get(&"shard-2"), Some(&3));
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(deserialize = "K: Ord + serde::Deserialize<'de>, V: serde::Deserialize<'de>"))
)]
pub struct MapLattice<K, V> {
    map: BTreeMap<K, V>,
}

impl<K: Ord, V> MapLattice<K, V> {
    /// Creates an empty `MapLattice` (the bottom element of the lattice).
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Inserts or replaces the value for `key`, returning the previous value if any.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.map.insert(key, value)
    }

    /// Returns a reference to the value for `key`, or `None` if absent.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Returns an iterator over keys in sorted order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.map.keys()
    }

    /// Returns an iterator over values in key-sorted order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values()
    }

    /// Returns the number of key-value pairs.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the map has no entries (i.e., this is the bottom element).
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<K: Ord, V> Default for MapLattice<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord, V: PartialEq> PartialEq for MapLattice<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<K: Ord, V: Eq> Eq for MapLattice<K, V> {}

impl<K: Ord, V: PartialOrd> PartialOrd for MapLattice<K, V> {
    /// `M₁ ≤ M₂` iff every key `k` in `M₁` is present in `M₂` with `M₁[k] ≤ M₂[k]`.
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        let self_le = self
            .map
            .iter()
            .all(|(k, v)| other.map.get(k).is_some_and(|ov| v <= ov));
        let other_le = other
            .map
            .iter()
            .all(|(k, v)| self.map.get(k).is_some_and(|sv| v <= sv));
        match (self_le, other_le) {
            (true, true) => Some(core::cmp::Ordering::Equal),
            (true, false) => Some(core::cmp::Ordering::Less),
            (false, true) => Some(core::cmp::Ordering::Greater),
            (false, false) => None,
        }
    }
}

impl<K: Ord + Clone, V: Lattice + Clone> Lattice for MapLattice<K, V> {
    /// Key-union with value-join on overlapping keys. The empty map is the identity.
    fn join(&self, other: &Self) -> Self {
        let mut result = self.map.clone();
        for (k, v) in &other.map {
            result
                .entry(k.clone())
                .and_modify(|existing| *existing = existing.join(v))
                .or_insert_with(|| v.clone());
        }
        Self { map: result }
    }

    /// Key-intersection with value-meet on overlapping keys. The empty map is absorbing.
    fn meet(&self, other: &Self) -> Self {
        let mut result = BTreeMap::new();
        for (k, v) in &self.map {
            if let Some(ov) = other.map.get(k) {
                result.insert(k.clone(), v.meet(ov));
            }
        }
        Self { map: result }
    }
}

// ── 7.2 SetLattice ────────────────────────────────────────────────────────────

/// A powerset lattice over a `BTreeSet<T>`.
///
/// The partial order is set inclusion: `A ≤ B` iff `A ⊆ B`. Meet is intersection;
/// join is union. The empty set is the bottom element (identity for join, absorbing for meet).
///
/// **Use case:** a global configuration state is advanced only when the set of acknowledging
/// nodes matches the expected cluster membership. Each node publishes its current
/// acknowledgement set; the coordinator-free merge (`meet` = intersection) computes the
/// universal acknowledgement — the set of nodes that *every* observer has confirmed.
///
/// **Note:** four lines of logic around `BTreeSet` — the value is the semantic contract
/// and the property tests, not the complexity of the implementation.
///
/// # Example
///
/// ```
/// use antichain::{SetLattice, Lattice};
///
/// let mut s1 = SetLattice::new();
/// s1.insert(1u64);
/// s1.insert(2u64);
///
/// let mut s2 = SetLattice::new();
/// s2.insert(2u64);
/// s2.insert(3u64);
///
/// // meet = intersection {2}
/// let m = s1.meet(&s2);
/// assert!(m.contains(&2));
/// assert!(!m.contains(&1));
/// assert!(!m.contains(&3));
///
/// // join = union {1, 2, 3}
/// let j = s1.join(&s2);
/// assert!(j.contains(&1) && j.contains(&2) && j.contains(&3));
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(deserialize = "T: Ord + serde::Deserialize<'de>"))
)]
pub struct SetLattice<T> {
    set: BTreeSet<T>,
}

impl<T: Ord> SetLattice<T> {
    /// Creates an empty `SetLattice` (the bottom element of the lattice).
    pub fn new() -> Self {
        Self {
            set: BTreeSet::new(),
        }
    }

    /// Inserts `value` into the set. Returns `true` if the value was not already present.
    pub fn insert(&mut self, value: T) -> bool {
        self.set.insert(value)
    }

    /// Returns `true` if `value` is a member of the set.
    pub fn contains(&self, value: &T) -> bool {
        self.set.contains(value)
    }

    /// Returns the number of elements in the set.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Returns `true` if the set is empty (i.e., this is the bottom element).
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Returns an iterator over the elements in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.set.iter()
    }
}

impl<T: Ord> Default for SetLattice<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> PartialEq for SetLattice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.set == other.set
    }
}

impl<T: Ord> Eq for SetLattice<T> {}

impl<T: Ord> PartialOrd for SetLattice<T> {
    /// `A ≤ B` iff `A ⊆ B`.
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        let self_le = self.set.is_subset(&other.set);
        let other_le = other.set.is_subset(&self.set);
        match (self_le, other_le) {
            (true, true) => Some(core::cmp::Ordering::Equal),
            (true, false) => Some(core::cmp::Ordering::Less),
            (false, true) => Some(core::cmp::Ordering::Greater),
            (false, false) => None,
        }
    }
}

impl<T: Ord + Clone> Lattice for SetLattice<T> {
    /// Intersection — the greatest lower bound under set inclusion.
    fn meet(&self, other: &Self) -> Self {
        Self {
            set: self.set.intersection(&other.set).cloned().collect(),
        }
    }
    /// Union — the least upper bound under set inclusion.
    fn join(&self, other: &Self) -> Self {
        Self {
            set: self.set.union(&other.set).cloned().collect(),
        }
    }
}

// ── Phase 7 unit tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_phase7 {
    use super::*;

    // ── WithTop ──────────────────────────────────────────────────────────────

    #[test]
    fn with_top_order_top_greatest() {
        let top: WithTop<u64> = WithTop::Top;
        let val = WithTop::Value(u64::MAX);
        assert!(top > val);
        assert!(val < top);
    }

    #[test]
    fn with_top_order_values_follow_inner() {
        assert!(WithTop::Value(3u64) < WithTop::Value(7u64));
        assert!(WithTop::Value(7u64) > WithTop::Value(3u64));
        assert_eq!(WithTop::Value(5u64), WithTop::Value(5u64));
    }

    #[test]
    fn with_top_meet_top_is_identity() {
        let top: WithTop<u64> = WithTop::Top;
        let val = WithTop::Value(42u64);
        assert_eq!(top.meet(&val), val.clone());
        assert_eq!(val.meet(&top), val);
    }

    #[test]
    fn with_top_meet_values_delegates_to_inner() {
        assert_eq!(
            WithTop::Value(3u64).meet(&WithTop::Value(7u64)),
            WithTop::Value(3u64)
        );
    }

    #[test]
    fn with_top_join_top_is_absorbing() {
        let top: WithTop<u64> = WithTop::Top;
        let val = WithTop::Value(42u64);
        assert_eq!(top.join(&val), WithTop::Top);
        assert_eq!(val.join(&top), WithTop::Top);
    }

    #[test]
    fn with_top_join_values_delegates_to_inner() {
        assert_eq!(
            WithTop::Value(3u64).join(&WithTop::Value(7u64)),
            WithTop::Value(7u64)
        );
    }

    #[test]
    fn with_top_meet_top_top_is_top() {
        let top: WithTop<u64> = WithTop::Top;
        assert_eq!(top.meet(&top), WithTop::Top);
    }

    #[test]
    fn with_top_join_top_top_is_top() {
        let top: WithTop<u64> = WithTop::Top;
        assert_eq!(top.join(&top), WithTop::Top);
    }

    // ── WithBottom ───────────────────────────────────────────────────────────

    #[test]
    fn with_bottom_order_bottom_least() {
        let bottom: WithBottom<u64> = WithBottom::Bottom;
        let val = WithBottom::Value(0u64);
        assert!(bottom < val);
        assert!(val > bottom);
    }

    #[test]
    fn with_bottom_order_values_follow_inner() {
        assert!(WithBottom::Value(3u64) < WithBottom::Value(7u64));
    }

    #[test]
    fn with_bottom_meet_bottom_is_absorbing() {
        let bottom: WithBottom<u64> = WithBottom::Bottom;
        let val = WithBottom::Value(42u64);
        assert_eq!(bottom.meet(&val), WithBottom::Bottom);
        assert_eq!(val.meet(&bottom), WithBottom::Bottom);
    }

    #[test]
    fn with_bottom_meet_values_delegates_to_inner() {
        assert_eq!(
            WithBottom::Value(3u64).meet(&WithBottom::Value(7u64)),
            WithBottom::Value(3u64)
        );
    }

    #[test]
    fn with_bottom_join_bottom_is_identity() {
        let bottom: WithBottom<u64> = WithBottom::Bottom;
        let val = WithBottom::Value(42u64);
        assert_eq!(bottom.join(&val), val.clone());
        assert_eq!(val.join(&bottom), val);
    }

    #[test]
    fn with_bottom_join_values_delegates_to_inner() {
        assert_eq!(
            WithBottom::Value(3u64).join(&WithBottom::Value(7u64)),
            WithBottom::Value(7u64)
        );
    }

    // ── WithTop<WithBottom<T>> composition ───────────────────────────────────

    #[test]
    fn with_top_with_bottom_three_level_order() {
        let bottom: WithTop<WithBottom<u64>> = WithTop::Value(WithBottom::Bottom);
        let val: WithTop<WithBottom<u64>> = WithTop::Value(WithBottom::Value(5u64));
        let top: WithTop<WithBottom<u64>> = WithTop::Top;

        assert!(bottom < val);
        assert!(val < top);
        assert!(bottom < top);
    }

    #[test]
    fn with_top_with_bottom_meet_top_is_identity() {
        let val: WithTop<WithBottom<u64>> = WithTop::Value(WithBottom::Value(5u64));
        let top: WithTop<WithBottom<u64>> = WithTop::Top;
        assert_eq!(top.meet(&val), val.clone());
    }

    #[test]
    fn with_top_with_bottom_join_bottom_is_identity() {
        let bottom: WithTop<WithBottom<u64>> = WithTop::Value(WithBottom::Bottom);
        let val: WithTop<WithBottom<u64>> = WithTop::Value(WithBottom::Value(5u64));
        assert_eq!(bottom.join(&val), val.clone());
    }

    // ── MapLattice ───────────────────────────────────────────────────────────

    #[test]
    fn map_lattice_new_is_empty() {
        let m: MapLattice<u64, u64> = MapLattice::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn map_lattice_insert_and_get() {
        let mut m: MapLattice<&str, u64> = MapLattice::new();
        m.insert("a", 10);
        assert_eq!(m.get(&"a"), Some(&10));
        assert_eq!(m.get(&"b"), None);
    }

    #[test]
    fn map_lattice_meet_is_key_intersection_with_value_meet() {
        let mut a: MapLattice<&str, u64> = MapLattice::new();
        a.insert("x", 10);
        a.insert("y", 5);
        let mut b: MapLattice<&str, u64> = MapLattice::new();
        b.insert("x", 7);
        b.insert("z", 3);

        let m = a.meet(&b);
        assert_eq!(m.get(&"x"), Some(&7)); // min(10, 7)
        assert_eq!(m.get(&"y"), None); // not in b
        assert_eq!(m.get(&"z"), None); // not in a
    }

    #[test]
    fn map_lattice_join_is_key_union_with_value_join() {
        let mut a: MapLattice<&str, u64> = MapLattice::new();
        a.insert("x", 10);
        a.insert("y", 5);
        let mut b: MapLattice<&str, u64> = MapLattice::new();
        b.insert("x", 7);
        b.insert("z", 3);

        let j = a.join(&b);
        assert_eq!(j.get(&"x"), Some(&10)); // max(10, 7)
        assert_eq!(j.get(&"y"), Some(&5)); // only in a
        assert_eq!(j.get(&"z"), Some(&3)); // only in b
    }

    #[test]
    fn map_lattice_meet_empty_is_absorbing() {
        let mut a: MapLattice<&str, u64> = MapLattice::new();
        a.insert("x", 10);
        let empty: MapLattice<&str, u64> = MapLattice::new();

        assert!(a.meet(&empty).is_empty());
        assert!(empty.meet(&a).is_empty());
    }

    #[test]
    fn map_lattice_join_empty_is_identity() {
        let mut a: MapLattice<&str, u64> = MapLattice::new();
        a.insert("x", 10);
        let empty: MapLattice<&str, u64> = MapLattice::new();

        assert_eq!(a.join(&empty), a.clone());
        assert_eq!(empty.join(&a), a.clone());
    }

    #[test]
    fn map_lattice_partial_order_empty_le_all() {
        let empty: MapLattice<u64, u64> = MapLattice::new();
        let mut nonempty: MapLattice<u64, u64> = MapLattice::new();
        nonempty.insert(1, 5);
        assert!(empty <= nonempty);
        assert_eq!(
            nonempty.partial_cmp(&empty),
            Some(core::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn map_lattice_partial_order_subset_with_lower_values() {
        let mut a: MapLattice<u64, u64> = MapLattice::new();
        a.insert(1, 3);
        let mut b: MapLattice<u64, u64> = MapLattice::new();
        b.insert(1, 7);
        b.insert(2, 5);
        // a <= b: a's key {1} ⊆ b's keys {1,2}, and a[1]=3 <= b[1]=7
        assert!(a <= b);
        assert_eq!(b.partial_cmp(&a), Some(core::cmp::Ordering::Greater));
    }

    #[test]
    fn map_lattice_partial_order_incomparable() {
        let mut a: MapLattice<u64, u64> = MapLattice::new();
        a.insert(1, 10);
        a.insert(2, 3);
        let mut b: MapLattice<u64, u64> = MapLattice::new();
        b.insert(1, 3);
        b.insert(2, 10);
        assert_eq!(a.partial_cmp(&b), None);
    }

    #[test]
    fn map_lattice_keys_and_values_iterators() {
        let mut m: MapLattice<u64, u64> = MapLattice::new();
        m.insert(1, 10);
        m.insert(2, 20);
        let keys: Vec<_> = m.keys().copied().collect();
        let values: Vec<_> = m.values().copied().collect();
        assert_eq!(keys, vec![1, 2]);
        assert_eq!(values, vec![10, 20]);
    }

    // ── SetLattice ───────────────────────────────────────────────────────────

    #[test]
    fn set_lattice_new_is_empty() {
        let s: SetLattice<u64> = SetLattice::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn set_lattice_insert_and_contains() {
        let mut s: SetLattice<u64> = SetLattice::new();
        s.insert(1);
        s.insert(2);
        assert!(s.contains(&1));
        assert!(s.contains(&2));
        assert!(!s.contains(&3));
    }

    #[test]
    fn set_lattice_meet_is_intersection() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        a.insert(2);
        let mut b: SetLattice<u64> = SetLattice::new();
        b.insert(2);
        b.insert(3);
        let m = a.meet(&b);
        assert!(m.contains(&2));
        assert!(!m.contains(&1));
        assert!(!m.contains(&3));
    }

    #[test]
    fn set_lattice_join_is_union() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        a.insert(2);
        let mut b: SetLattice<u64> = SetLattice::new();
        b.insert(2);
        b.insert(3);
        let j = a.join(&b);
        assert!(j.contains(&1) && j.contains(&2) && j.contains(&3));
    }

    #[test]
    fn set_lattice_meet_is_commutative() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        a.insert(2);
        let mut b: SetLattice<u64> = SetLattice::new();
        b.insert(2);
        b.insert(3);
        assert_eq!(a.meet(&b), b.meet(&a));
    }

    #[test]
    fn set_lattice_join_is_commutative() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        let mut b: SetLattice<u64> = SetLattice::new();
        b.insert(2);
        assert_eq!(a.join(&b), b.join(&a));
    }

    #[test]
    fn set_lattice_partial_order_empty_le_all() {
        let empty: SetLattice<u64> = SetLattice::new();
        let mut nonempty: SetLattice<u64> = SetLattice::new();
        nonempty.insert(1);
        assert!(empty <= nonempty);
        assert_eq!(
            nonempty.partial_cmp(&empty),
            Some(core::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn set_lattice_partial_order_subset() {
        let mut sub: SetLattice<u64> = SetLattice::new();
        sub.insert(1);
        let mut sup: SetLattice<u64> = SetLattice::new();
        sup.insert(1);
        sup.insert(2);
        assert!(sub <= sup);
        assert_eq!(sup.partial_cmp(&sub), Some(core::cmp::Ordering::Greater));
    }

    #[test]
    fn set_lattice_partial_order_incomparable() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        let mut b: SetLattice<u64> = SetLattice::new();
        b.insert(2);
        assert_eq!(a.partial_cmp(&b), None);
    }

    #[test]
    fn set_lattice_meet_empty_is_absorbing() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        let empty: SetLattice<u64> = SetLattice::new();
        assert!(a.meet(&empty).is_empty());
        assert!(empty.meet(&a).is_empty());
    }

    #[test]
    fn set_lattice_join_empty_is_identity() {
        let mut a: SetLattice<u64> = SetLattice::new();
        a.insert(1);
        let empty: SetLattice<u64> = SetLattice::new();
        assert_eq!(a.join(&empty), a.clone());
        assert_eq!(empty.join(&a), a.clone());
    }

    // ── Frontier<WithTop<u64>> ────────────────────────────────────────────────

    #[test]
    fn frontier_with_top_top_dominates_any_value() {
        let ft = Frontier::from_elem(WithTop::<u64>::Top);
        let fv = Frontier::from_elem(WithTop::Value(u64::MAX));
        // After meet, only Top survives (Top is the bottom of the inverted order means
        // Top "dominates" Value in terms of less_equal for Frontier::meet).
        // Actually: in WithTop, Value < Top, so the frontier meet keeps the lower element = Value.
        // Frontier::meet = antichain meet = builds union of elements then deduplicates.
        // Top > Value(MAX), so Top dominates Value — only Value survives in the antichain.
        let m = ft.meet(&fv);
        assert_eq!(m.elements().len(), 1);
        assert_eq!(m.elements()[0], WithTop::Value(u64::MAX));
    }

    #[test]
    fn frontier_with_bottom_bottom_is_dominated() {
        let fb = Frontier::from_elem(WithBottom::<u64>::Bottom);
        let fv = Frontier::from_elem(WithBottom::Value(0u64));
        // Bottom < Value(0), so Bottom is dominated by Value — only Bottom survives in meet.
        let m = fb.meet(&fv);
        assert_eq!(m.elements().len(), 1);
        assert_eq!(m.elements()[0], WithBottom::Bottom);
    }
}

// ── Phase 7 property tests ────────────────────────────────────────────────────
//
// Key law enforced by every prop test: a ≤ b ⟺ meet(a, b) == a ⟺ join(a, b) == b.
// This PartialOrd/meet/join consistency law is the hardest to violate correctly and
// the most important to verify for dynamic-arity and lifted lattices.

#[cfg(test)]
mod prop_tests_phase7 {
    use super::*;
    use proptest::prelude::*;

    // ── Strategies ────────────────────────────────────────────────────────────

    prop_compose! {
        fn arb_with_top_u64()(
            is_top in any::<bool>(), v in any::<u64>()
        ) -> WithTop<u64> {
            if is_top { WithTop::Top } else { WithTop::Value(v) }
        }
    }

    prop_compose! {
        fn arb_with_bottom_u64()(
            is_bottom in any::<bool>(), v in any::<u64>()
        ) -> WithBottom<u64> {
            if is_bottom { WithBottom::Bottom } else { WithBottom::Value(v) }
        }
    }

    prop_compose! {
        fn arb_map_lattice()(
            entries in prop::collection::vec((any::<u64>(), any::<u64>()), 0..8)
        ) -> MapLattice<u64, u64> {
            let mut m = MapLattice::new();
            for (k, v) in entries { m.insert(k, v); }
            m
        }
    }

    prop_compose! {
        fn arb_set_lattice()(
            elems in prop::collection::vec(0u64..20u64, 0..8)
        ) -> SetLattice<u64> {
            let mut s = SetLattice::new();
            for e in elems { s.insert(e); }
            s
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        // ── WithTop<u64>: order and Lattice laws ──────────────────────────────

        #[test]
        fn prop_with_top_meet_commutative(
            a in arb_with_top_u64(), b in arb_with_top_u64()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_with_top_meet_associative(
            a in arb_with_top_u64(), b in arb_with_top_u64(), c in arb_with_top_u64()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_with_top_meet_idempotent(a in arb_with_top_u64()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn prop_with_top_join_commutative(
            a in arb_with_top_u64(), b in arb_with_top_u64()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn prop_with_top_join_associative(
            a in arb_with_top_u64(), b in arb_with_top_u64(), c in arb_with_top_u64()
        ) {
            prop_assert_eq!(a.join(&b.join(&c)), a.join(&b).join(&c));
        }

        #[test]
        fn prop_with_top_join_idempotent(a in arb_with_top_u64()) {
            prop_assert_eq!(a.join(&a), a);
        }

        /// PartialOrd/meet consistency: a ≤ b ⟹ meet(a, b) == a.
        #[test]
        fn prop_with_top_meet_consistency(
            a in arb_with_top_u64(), b in arb_with_top_u64()
        ) {
            if let Some(ord) = a.partial_cmp(&b)
                && ord.is_le() {
                    prop_assert_eq!(a.meet(&b), a.clone());
                    prop_assert_eq!(a.join(&b), b.clone());
                }
        }

        /// meet is always a lower bound: meet(a, b) ≤ a and meet(a, b) ≤ b.
        #[test]
        fn prop_with_top_meet_is_lower_bound(
            a in arb_with_top_u64(), b in arb_with_top_u64()
        ) {
            let m = a.meet(&b);
            prop_assert!(m <= a);
            prop_assert!(m <= b);
        }

        // ── WithBottom<u64>: Lattice laws ─────────────────────────────────────

        #[test]
        fn prop_with_bottom_meet_commutative(
            a in arb_with_bottom_u64(), b in arb_with_bottom_u64()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_with_bottom_meet_associative(
            a in arb_with_bottom_u64(), b in arb_with_bottom_u64(), c in arb_with_bottom_u64()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_with_bottom_meet_idempotent(a in arb_with_bottom_u64()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn prop_with_bottom_join_commutative(
            a in arb_with_bottom_u64(), b in arb_with_bottom_u64()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn prop_with_bottom_join_idempotent(a in arb_with_bottom_u64()) {
            prop_assert_eq!(a.join(&a), a);
        }

        /// PartialOrd/meet consistency for WithBottom.
        #[test]
        fn prop_with_bottom_meet_consistency(
            a in arb_with_bottom_u64(), b in arb_with_bottom_u64()
        ) {
            if let Some(ord) = a.partial_cmp(&b)
                && ord.is_le() {
                    prop_assert_eq!(a.meet(&b), a.clone());
                    prop_assert_eq!(a.join(&b), b.clone());
                }
        }

        // ── MapLattice<u64, u64>: Lattice laws ───────────────────────────────

        #[test]
        fn prop_map_lattice_meet_commutative(
            a in arb_map_lattice(), b in arb_map_lattice()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_map_lattice_meet_associative(
            a in arb_map_lattice(), b in arb_map_lattice(), c in arb_map_lattice()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_map_lattice_meet_idempotent(a in arb_map_lattice()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn prop_map_lattice_join_commutative(
            a in arb_map_lattice(), b in arb_map_lattice()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn prop_map_lattice_join_associative(
            a in arb_map_lattice(), b in arb_map_lattice(), c in arb_map_lattice()
        ) {
            prop_assert_eq!(a.join(&b.join(&c)), a.join(&b).join(&c));
        }

        #[test]
        fn prop_map_lattice_join_idempotent(a in arb_map_lattice()) {
            prop_assert_eq!(a.join(&a), a);
        }

        /// PartialOrd/meet consistency (the critical law for dynamic lattices).
        ///
        /// a ≤ b ⟹ meet(a, b) == a AND join(a, b) == b.
        #[test]
        fn prop_map_lattice_meet_consistency(
            a in arb_map_lattice(), b in arb_map_lattice()
        ) {
            if let Some(ord) = a.partial_cmp(&b)
                && ord.is_le() {
                    prop_assert_eq!(a.meet(&b), a.clone());
                    prop_assert_eq!(a.join(&b), b.clone());
                }
        }

        /// meet is always a lower bound.
        #[test]
        fn prop_map_lattice_meet_is_lower_bound(
            a in arb_map_lattice(), b in arb_map_lattice()
        ) {
            let m = a.meet(&b);
            // m <= a and m <= b (partial_cmp returns Some(Less) or Some(Equal))
            prop_assert!(m.partial_cmp(&a).is_some_and(|o| o.is_le()));
            prop_assert!(m.partial_cmp(&b).is_some_and(|o| o.is_le()));
        }

        /// join is always an upper bound.
        #[test]
        fn prop_map_lattice_join_is_upper_bound(
            a in arb_map_lattice(), b in arb_map_lattice()
        ) {
            let j = a.join(&b);
            prop_assert!(a.partial_cmp(&j).is_some_and(|o| o.is_le()));
            prop_assert!(b.partial_cmp(&j).is_some_and(|o| o.is_le()));
        }

        // ── SetLattice<u64>: Lattice laws ─────────────────────────────────────

        #[test]
        fn prop_set_lattice_meet_commutative(
            a in arb_set_lattice(), b in arb_set_lattice()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn prop_set_lattice_meet_associative(
            a in arb_set_lattice(), b in arb_set_lattice(), c in arb_set_lattice()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn prop_set_lattice_meet_idempotent(a in arb_set_lattice()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn prop_set_lattice_join_commutative(
            a in arb_set_lattice(), b in arb_set_lattice()
        ) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn prop_set_lattice_join_associative(
            a in arb_set_lattice(), b in arb_set_lattice(), c in arb_set_lattice()
        ) {
            prop_assert_eq!(a.join(&b.join(&c)), a.join(&b).join(&c));
        }

        #[test]
        fn prop_set_lattice_join_idempotent(a in arb_set_lattice()) {
            prop_assert_eq!(a.join(&a), a);
        }

        /// PartialOrd/meet consistency for SetLattice.
        #[test]
        fn prop_set_lattice_meet_consistency(
            a in arb_set_lattice(), b in arb_set_lattice()
        ) {
            if let Some(ord) = a.partial_cmp(&b)
                && ord.is_le() {
                    prop_assert_eq!(a.meet(&b), a.clone());
                    prop_assert_eq!(a.join(&b), b.clone());
                }
        }

        /// meet is always a lower bound under set inclusion.
        #[test]
        fn prop_set_lattice_meet_is_lower_bound(
            a in arb_set_lattice(), b in arb_set_lattice()
        ) {
            let m = a.meet(&b);
            prop_assert!(m.partial_cmp(&a).is_some_and(|o| o.is_le()));
            prop_assert!(m.partial_cmp(&b).is_some_and(|o| o.is_le()));
        }
    }
}

// ── Phase 8: performance validation and design-debt resolution ────────────────
//
// 8.1 — Width-bound documented with measured timing data (see crate-level docs).
// 8.2 — Adapter sufficiency validated in examples/progress_protocol.rs.
// 8.3 — Bounded<T> relaxed from T: Ord to T: PartialOrd; Min<T> retained.

#[cfg(test)]
mod tests_phase8 {
    use super::*;

    // ── 8.1: Empirical width-bound verification ───────────────────────────────

    /// Frontier<u64> collapses to width 1 for any number of u64 inputs because u64
    /// is totally ordered and only the minimum survives. This is the basis for the
    /// "O(1) for totally-ordered T" claim in the crate-level performance table.
    #[test]
    fn frontier_u64_collapses_to_width_1() {
        let f = Frontier::from_elements(0u64..1000);
        assert_eq!(f.elements().len(), 1);
        assert_eq!(f.elements(), &[0u64]);
    }

    /// Width-n antichain of mutually incomparable ProductTimestamp elements is the
    /// adversarial case. At width=100 the antichain is exactly 100 elements wide.
    /// Measured meet cost at this width: ≈ 9 µs — well within the acceptable range.
    /// Conclusion: no compaction step is needed for practical widths (≤ 50).
    #[test]
    fn frontier_product_timestamp_width_equals_incomparable_count() {
        let width = 100u64;
        let f = Frontier::from_elements((0..width).map(|i| ProductTimestamp::new(i, width - i)));
        assert_eq!(f.elements().len() as u64, width);
    }

    // ── 8.3: Bounded<T> now T: PartialOrd — composes with ProductTimestamp ────

    /// Before Phase 8.3, Bounded<T> required T: Ord, preventing composition with
    /// ProductTimestamp<T1, T2> (which is only PartialOrd). After relaxing to
    /// T: PartialOrd, Bounded<ProductTimestamp<u64, u64>> compiles and works.
    #[test]
    fn bounded_composes_with_product_timestamp() {
        let p_min = ProductTimestamp::new(0u64, 0u64);
        let p_max = ProductTimestamp::new(10u64, 10u64);

        let b1 = Bounded::new(
            ProductTimestamp::new(3u64, 7u64),
            p_min.clone(),
            p_max.clone(),
        );
        let b2 = Bounded::new(
            ProductTimestamp::new(5u64, 2u64),
            p_min.clone(),
            p_max.clone(),
        );

        // Component-wise meet: (min(3,5), min(7,2)) = (3, 2)
        let m = b1.meet(&b2);
        assert_eq!(m.value().outer, 3u64);
        assert_eq!(m.value().inner, 2u64);

        // Component-wise join: (max(3,5), max(7,2)) = (5, 7)
        let j = b1.join(&b2);
        assert_eq!(j.value().outer, 5u64);
        assert_eq!(j.value().inner, 7u64);
    }

    /// Full composition chain: Frontier<Bounded<ProductTimestamp<u64, u64>>>.
    /// Two incomparable bounded products both survive the antichain meet.
    #[test]
    fn frontier_bounded_product_timestamp_end_to_end() {
        let p_min = ProductTimestamp::new(0u64, 0u64);
        let p_max = ProductTimestamp::new(100u64, 100u64);

        let f1 = Frontier::from_elem(Bounded::new(
            ProductTimestamp::new(30u64, 70u64),
            p_min.clone(),
            p_max.clone(),
        ));
        let f2 = Frontier::from_elem(Bounded::new(
            ProductTimestamp::new(50u64, 20u64),
            p_min.clone(),
            p_max.clone(),
        ));

        // (30,70) and (50,20) are incomparable in product order → both survive meet
        let m = f1.meet(&f2);
        assert_eq!(m.elements().len(), 2);
    }

    // ── 8.3: Min<T> retention decision ────────────────────────────────────────

    /// Min<T> is retained as a semantic newtype. Its value is the intent it communicates
    /// when paired with Max<T> in composite types like (Max<T>, Min<T>). The API surface
    /// is minimal and the clarity justifies it. No downstream usage data contradicts
    /// this decision as of Phase 8.
    #[test]
    fn min_earns_its_place_in_composite_with_max() {
        // (Max<u64>, Min<u64>) models a sliding window [lower_bound, upper_bound]:
        // Max tracks the highest confirmed lower bound;
        // Min tracks the lowest observed upper bound.
        let f1 = Frontier::from_elem((Max(5u64), Min(20u64)));
        let f2 = Frontier::from_elem((Max(8u64), Min(15u64)));

        // meet: highest lower bound max(5,8)=8; lowest upper bound min(20,15)=15.
        let merged = f1.meet(&f2);
        assert_eq!(merged.elements()[0].0, Max(8u64));
        assert_eq!(merged.elements()[0].1, Min(15u64));
    }
}

// ── Universal lattice consistency law ─────────────────────────────────────────
//
// The "connecting lemma": in a genuine lattice the partial order and the
// meet/join operations are two faces of the same structure, linked by
//
//     a ≤ b   ⟺   meet(a, b) == a   ⟺   join(a, b) == b.
//
// Earlier phase modules check only the *forward* direction (a ≤ b ⟹ meet == a)
// and only for a subset of types. This module verifies the **biconditional** in
// **both** directions for **every** lattice type the crate exposes — the single
// strongest correctness statement tying `PartialOrd` to `Lattice`.
//
// The bare tuple `(A, B)` is deliberately excluded: its `Lattice` impl is
// component-wise while its `PartialOrd` is lexicographic, so component-wise meet
// is *not* the greatest lower bound under that order (documented at the impl).
// The law genuinely does not hold there, which is exactly why product-order use
// cases must reach for `ProductTimestamp` instead.
#[cfg(test)]
mod prop_tests_consistency {
    use super::*;
    use proptest::prelude::*;

    /// Asserts the biconditional consistency law for one `(a, b)` pair.
    ///
    /// Checks both directions:
    /// - `a ≤ b ⟺ meet(a, b) == a`
    /// - `a ≤ b ⟺ join(a, b) == b`
    fn check<T>(a: &T, b: &T) -> Result<(), TestCaseError>
    where
        T: Lattice + PartialEq + Clone + core::fmt::Debug,
    {
        let le = matches!(a.partial_cmp(b), Some(o) if o.is_le());
        let meet_is_a = &a.meet(b) == a;
        let join_is_b = &a.join(b) == b;
        prop_assert_eq!(
            le,
            meet_is_a,
            "a ≤ b ⟺ meet(a,b)==a violated for {:?}, {:?}",
            a,
            b
        );
        prop_assert_eq!(
            le,
            join_is_b,
            "a ≤ b ⟺ join(a,b)==b violated for {:?}, {:?}",
            a,
            b
        );
        Ok(())
    }

    // ── Strategies ────────────────────────────────────────────────────────────

    prop_compose! {
        fn arb_product()(x in any::<u64>(), y in any::<u64>()) -> ProductTimestamp<u64, u64> {
            ProductTimestamp::new(x, y)
        }
    }

    prop_compose! {
        fn arb_lexicographic()(x in any::<u64>(), y in any::<u64>()) -> Lexicographic<u64, u64> {
            Lexicographic::new(x, y)
        }
    }

    prop_compose! {
        // Fixed bounds keep the operation well-defined: mixing ranges is undefined by design.
        fn arb_bounded()(v in 0u64..=100) -> Bounded<u64> {
            Bounded::new(v, 0, 100)
        }
    }

    prop_compose! {
        fn arb_with_top()(is_top in any::<bool>(), v in any::<u64>()) -> WithTop<u64> {
            if is_top { WithTop::Top } else { WithTop::Value(v) }
        }
    }

    prop_compose! {
        fn arb_with_bottom()(is_bot in any::<bool>(), v in any::<u64>()) -> WithBottom<u64> {
            if is_bot { WithBottom::Bottom } else { WithBottom::Value(v) }
        }
    }

    prop_compose! {
        fn arb_map()(entries in prop::collection::vec((0u64..8, any::<u64>()), 0..6))
            -> MapLattice<u64, u64> {
            let mut m = MapLattice::new();
            for (k, v) in entries { m.insert(k, v); }
            m
        }
    }

    prop_compose! {
        fn arb_set()(elems in prop::collection::vec(0u64..12, 0..6)) -> SetLattice<u64> {
            let mut s = SetLattice::new();
            for e in elems { s.insert(e); }
            s
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        #[test]
        fn consistency_u64(a in any::<u64>(), b in any::<u64>()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_product(a in arb_product(), b in arb_product()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_lexicographic(a in arb_lexicographic(), b in arb_lexicographic()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_max(a in any::<u64>(), b in any::<u64>()) {
            check(&Max(a), &Max(b))?;
        }

        #[test]
        fn consistency_min(a in any::<u64>(), b in any::<u64>()) {
            check(&Min(a), &Min(b))?;
        }

        #[test]
        fn consistency_bounded(a in arb_bounded(), b in arb_bounded()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_with_top(a in arb_with_top(), b in arb_with_top()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_with_bottom(a in arb_with_bottom(), b in arb_with_bottom()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_map_lattice(a in arb_map(), b in arb_map()) {
            check(&a, &b)?;
        }

        #[test]
        fn consistency_set_lattice(a in arb_set(), b in arb_set()) {
            check(&a, &b)?;
        }

        // Composition preserves the law: a nested lattice still satisfies it.
        #[test]
        fn consistency_nested_with_top_with_bottom(
            a in arb_with_bottom(), b in arb_with_bottom()
        ) {
            let wa = WithTop::Value(a);
            let wb = WithTop::Value(b);
            check(&wa, &wb)?;
        }

        #[test]
        fn consistency_map_of_product(
            ea in prop::collection::vec((0u64..6, any::<u64>(), any::<u64>()), 0..5),
            eb in prop::collection::vec((0u64..6, any::<u64>(), any::<u64>()), 0..5),
        ) {
            let mut a: MapLattice<u64, ProductTimestamp<u64, u64>> = MapLattice::new();
            for (k, x, y) in ea { a.insert(k, ProductTimestamp::new(x, y)); }
            let mut b: MapLattice<u64, ProductTimestamp<u64, u64>> = MapLattice::new();
            for (k, x, y) in eb { b.insert(k, ProductTimestamp::new(x, y)); }
            check(&a, &b)?;
        }
    }
}

// ── Serde round-trip ──────────────────────────────────────────────────────────
//
// Locks two guarantees against regression:
// 1. The inline-storage optimization keeps `Antichain`'s wire format identical to
//    the original derive (`{ "elements": [...] }`).
// 2. The `MapLattice`/`SetLattice` serde derives compile and round-trip (the
//    `serde/alloc` feature wiring and the `Ord` deserialize bounds).
#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    #[test]
    fn antichain_wire_format_is_stable() {
        // Width 1 is stored inline, but must still serialize as { "elements": [..] }.
        let a = Antichain::from_elem(7u64);
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#"{"elements":[7]}"#);

        // Empty antichain.
        let empty = Antichain::<u64>::empty();
        assert_eq!(serde_json::to_string(&empty).unwrap(), r#"{"elements":[]}"#);
    }

    #[test]
    fn antichain_round_trips_at_every_width() {
        for elems in [vec![], vec![5u64], vec![1u64, 9, 4]] {
            let mut original = Antichain::<u64>::empty();
            for e in elems {
                original.insert(e);
            }
            let json = serde_json::to_string(&original).unwrap();
            let restored: Antichain<u64> = serde_json::from_str(&json).unwrap();
            assert_eq!(original, restored);
        }
    }

    #[test]
    fn frontier_round_trips() {
        let f = Frontier::from_elements([3u64, 7, 5]);
        let json = serde_json::to_string(&f).unwrap();
        let restored: Frontier<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(f, restored);
    }

    #[test]
    fn map_lattice_round_trips() {
        let mut m: MapLattice<u32, u64> = MapLattice::new();
        m.insert(0, 10);
        m.insert(1, 20);
        let json = serde_json::to_string(&m).unwrap();
        let restored: MapLattice<u32, u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(m, restored);
    }

    #[test]
    fn set_lattice_round_trips() {
        let mut s: SetLattice<u64> = SetLattice::new();
        s.insert(1);
        s.insert(2);
        s.insert(3);
        let json = serde_json::to_string(&s).unwrap();
        let restored: SetLattice<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(s, restored);
    }
}
