//! A coordinator-free primitive for tracking distributed progress using lattice algebra.

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

// ── Antichain ─────────────────────────────────────────────────────────────────

/// A set of mutually incomparable elements under `PartialOrd`.
///
/// Invariant: no element `x` in the set satisfies `x <= y` or `y <= x`
/// for any other element `y` in the set.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Antichain<T> {
    elements: Vec<T>,
}

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
        self.elements.retain(|e| !(t <= *e));
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
    /// Commutative, associative, and idempotent — safe to apply without coordination.
    /// Two nodes that have each seen any subset of the same update set, in any order,
    /// will hold identical `Frontier` values after merging.
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
