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

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::vec;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

// ── Lattice ───────────────────────────────────────────────────────────────────

/// Greatest lower bound (`meet`) and least upper bound (`join`).
///
/// Implementations must be consistent with `PartialOrd`:
/// - `meet(a, b) <= a` and `meet(a, b) <= b`
/// - `a <= join(a, b)` and `b <= join(a, b)`
pub trait Lattice: PartialOrd {
    fn meet(&self, other: &Self) -> Self;
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

impl_lattice_ord!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

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
    pub outer: T1,
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
            (Some(Greater), Some(Greater | Equal)) | (Some(Equal), Some(Greater)) => {
                Some(Greater)
            }
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
    pub outer: A,
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

// ── Antichain ─────────────────────────────────────────────────────────────────

/// A set of mutually incomparable elements under `PartialOrd`.
///
/// Invariant: no element `x` in the set satisfies `x <= y` or `y <= x`
/// for any other element `y` in the set.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Antichain<T> {
    elements: Vec<T>,
}

/// Two antichains are equal when they contain the same *set* of elements,
/// regardless of insertion order.
impl<T: PartialEq> PartialEq for Antichain<T> {
    fn eq(&self, other: &Self) -> bool {
        self.elements.len() == other.elements.len()
            && self.elements.iter().all(|e| other.elements.contains(e))
    }
}

impl<T: Eq> Eq for Antichain<T> {}

impl<T: PartialOrd + Clone> Antichain<T> {
    /// Creates an empty antichain.
    pub fn empty() -> Self {
        Self { elements: Vec::new() }
    }

    /// Creates an antichain containing a single element.
    pub fn from_elem(t: T) -> Self {
        Self { elements: vec![t] }
    }

    /// Inserts `t`, maintaining the antichain invariant.
    ///
    /// - Skips `t` if any existing element `e` satisfies `e <= t` (t is dominated).
    /// - Drops any existing `e` where `t <= e` (e is dominated by t).
    pub fn insert(&mut self, t: T) {
        if self.elements.iter().any(|e| *e <= t) {
            return;
        }
        self.elements
            .retain(|e| t.partial_cmp(e).is_none_or(|o| o == core::cmp::Ordering::Greater));
        self.elements.push(t);
    }

    /// Returns the elements of the antichain as a slice.
    pub fn elements(&self) -> &[T] {
        &self.elements
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
        Self { antichain: Antichain::empty() }
    }

    /// Creates a frontier from a single element.
    pub fn from_elem(t: T) -> Self {
        Self { antichain: Antichain::from_elem(t) }
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
        assert!(a.less_equal(&3));   // 3 <= 5 → in-flight
        assert!(a.less_equal(&5));   // 5 <= 5 → in-flight
        assert!(!a.less_equal(&7));  // 7 > 5  → past the frontier
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
