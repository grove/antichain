//! Disjoint-interval-set lattice for tracking out-of-order progress with gaps.
//!
//! This is the companion crate to [`antichain`], implementing the Phase 7.4
//! `IntervalSetLattice` deferred from the core crate. It models progress that
//! arrives out of order and leaves *holes* — a backfill engine that has processed
//! blocks 150–200 while block 101 is still delayed, for example.
//!
//! # The lattice
//!
//! [`IntervalSetLattice<T>`] stores a canonical set of disjoint, non-adjacent,
//! half-open intervals `[start, end)`. The partial order is **set inclusion** over
//! the points covered:
//!
//! - `A ≤ B` iff every point covered by `A` is also covered by `B`.
//! - [`meet`](antichain::Lattice::meet) = **intersection** → the smallest commonly
//!   covered set (coordinator-free safe merge).
//! - [`join`](antichain::Lattice::join) = **union with coalescing** → the largest
//!   covered set.
//!
//! The empty set is the bottom element: the identity for `join` and absorbing for
//! `meet`. Because it implements [`antichain::Lattice`], an `IntervalSetLattice`
//! can be dropped straight into a [`antichain::Frontier`] or used as the value type
//! of a [`antichain::MapLattice`].
//!
//! # Example
//!
//! ```
//! use antichain_intervals::IntervalSetLattice;
//! use antichain::Lattice;
//!
//! // A backfill worker has covered blocks [100, 150) and [200, 250).
//! let mut a = IntervalSetLattice::new();
//! a.insert(100u64, 150);
//! a.insert(200, 250);
//!
//! // Another worker has covered [120, 210).
//! let mut b = IntervalSetLattice::new();
//! b.insert(120u64, 210);
//!
//! // Safe (coordinator-free) progress = intersection: what BOTH have covered.
//! let safe = a.meet(&b);
//! assert_eq!(safe.intervals(), &[(120, 150), (200, 210)]);
//!
//! // Optimistic coverage = union, with the touching ranges coalesced.
//! let seen = a.join(&b);
//! assert_eq!(seen.intervals(), &[(100, 250)]);
//! ```
//!
//! # `no_std`
//!
//! Disable the default `std` feature for `no_std` + `alloc` environments.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

use alloc::vec::Vec;
use antichain::Lattice;
use core::cmp::Ordering;

/// A canonical set of disjoint, non-adjacent half-open intervals `[start, end)`.
///
/// Invariants maintained at all times:
/// - every interval satisfies `start < end` (empty intervals are dropped);
/// - intervals are sorted by `start`;
/// - no two intervals overlap **or touch** (touching intervals are coalesced, so
///   `[1, 3)` and `[3, 5)` are always stored as `[1, 5)`).
///
/// These invariants make structural equality coincide with set equality, so
/// `PartialEq` answers "do these cover the same points?".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntervalSetLattice<T> {
    /// Canonical, sorted, coalesced `[start, end)` intervals.
    intervals: Vec<(T, T)>,
}

impl<T: Ord + Clone> IntervalSetLattice<T> {
    /// Creates an empty interval set (the bottom element of the lattice).
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
        }
    }

    /// Creates an interval set covering the single half-open interval `[start, end)`.
    ///
    /// An empty or inverted interval (`start >= end`) yields the empty set.
    pub fn from_interval(start: T, end: T) -> Self {
        let mut s = Self::new();
        s.insert(start, end);
        s
    }

    /// Adds the half-open interval `[start, end)`, re-canonicalizing the set.
    ///
    /// Overlapping and touching intervals are coalesced. An empty or inverted
    /// interval (`start >= end`) is ignored.
    pub fn insert(&mut self, start: T, end: T) {
        if start >= end {
            return;
        }
        self.intervals.push((start, end));
        self.canonicalize();
    }

    /// Returns `true` if `point` lies within any covered interval.
    pub fn contains(&self, point: &T) -> bool {
        self.intervals.iter().any(|(s, e)| s <= point && point < e)
    }

    /// Returns the canonical intervals as a slice of `(start, end)` pairs.
    pub fn intervals(&self) -> &[(T, T)] {
        &self.intervals
    }

    /// Returns the number of disjoint intervals (not the number of covered points).
    pub fn len(&self) -> usize {
        self.intervals.len()
    }

    /// Returns `true` if the set covers nothing.
    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    /// Sorts, then merges every overlapping or touching interval in place.
    fn canonicalize(&mut self) {
        self.intervals.sort_by(|a, b| a.0.cmp(&b.0));
        let mut merged: Vec<(T, T)> = Vec::with_capacity(self.intervals.len());
        for (s, e) in self.intervals.drain(..) {
            match merged.last_mut() {
                // Overlap or touch: `s <= prev_end` means they coalesce.
                Some(last) if s <= last.1 => {
                    if e > last.1 {
                        last.1 = e;
                    }
                }
                _ => merged.push((s, e)),
            }
        }
        self.intervals = merged;
    }

    /// Returns `true` if every point covered by `self` is also covered by `other`.
    fn is_subset_of(&self, other: &Self) -> bool {
        // Each canonical interval of `self` has no internal gap, so it must fit
        // entirely inside a single canonical interval of `other`.
        self.intervals
            .iter()
            .all(|(s, e)| other.intervals.iter().any(|(os, oe)| os <= s && e <= oe))
    }
}

impl<T: Ord + Clone> PartialOrd for IntervalSetLattice<T> {
    /// Set-inclusion order: `A ≤ B` iff `A ⊆ B`.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let le = self.is_subset_of(other);
        let ge = other.is_subset_of(self);
        match (le, ge) {
            (true, true) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            (false, false) => None,
        }
    }
}

impl<T: Ord + Clone> Lattice for IntervalSetLattice<T> {
    /// Intersection: the smallest set of points covered by **both** inputs.
    ///
    /// A classic two-pointer sweep over the two sorted, disjoint interval lists.
    fn meet(&self, other: &Self) -> Self {
        let mut out: Vec<(T, T)> = Vec::new();
        let (mut i, mut j) = (0usize, 0usize);
        while i < self.intervals.len() && j < other.intervals.len() {
            let (a_s, a_e) = &self.intervals[i];
            let (b_s, b_e) = &other.intervals[j];
            let start = a_s.max(b_s);
            let end = a_e.min(b_e);
            if start < end {
                out.push((start.clone(), end.clone()));
            }
            // Advance the interval that ends first.
            if a_e <= b_e {
                i += 1;
            } else {
                j += 1;
            }
        }
        // Inputs are already canonical and the sweep emits sorted, disjoint
        // intersections, so `out` is already canonical.
        Self { intervals: out }
    }

    /// Union with coalescing: the largest set of points covered by **either** input.
    fn join(&self, other: &Self) -> Self {
        let mut intervals = self.intervals.clone();
        intervals.extend(other.intervals.iter().cloned());
        let mut result = Self { intervals };
        result.canonicalize();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_coalesces_overlapping_and_touching() {
        let mut s = IntervalSetLattice::new();
        s.insert(1u64, 3);
        s.insert(3, 5); // touching → coalesce
        s.insert(10, 12); // disjoint
        s.insert(2, 4); // overlapping the first block
        assert_eq!(s.intervals(), &[(1, 5), (10, 12)]);
    }

    #[test]
    fn empty_and_inverted_intervals_are_ignored() {
        let mut s = IntervalSetLattice::new();
        s.insert(5u64, 5); // empty
        s.insert(8, 3); // inverted
        assert!(s.is_empty());
    }

    #[test]
    fn contains_respects_half_open_bounds() {
        let s = IntervalSetLattice::from_interval(10u64, 20);
        assert!(s.contains(&10));
        assert!(s.contains(&19));
        assert!(!s.contains(&20)); // half-open: end is excluded
        assert!(!s.contains(&9));
    }

    #[test]
    fn meet_is_intersection() {
        let mut a = IntervalSetLattice::new();
        a.insert(100u64, 150);
        a.insert(200, 250);
        let b = IntervalSetLattice::from_interval(120u64, 210);
        let m = a.meet(&b);
        assert_eq!(m.intervals(), &[(120, 150), (200, 210)]);
    }

    #[test]
    fn join_is_coalescing_union() {
        let mut a = IntervalSetLattice::new();
        a.insert(100u64, 150);
        a.insert(200, 250);
        let b = IntervalSetLattice::from_interval(120u64, 210);
        let j = a.join(&b);
        assert_eq!(j.intervals(), &[(100, 250)]);
    }

    #[test]
    fn empty_is_join_identity_and_meet_absorber() {
        let empty: IntervalSetLattice<u64> = IntervalSetLattice::new();
        let s = IntervalSetLattice::from_interval(1u64, 9);
        assert_eq!(empty.join(&s), s);
        assert_eq!(s.join(&empty), s);
        assert_eq!(empty.meet(&s), empty);
        assert_eq!(s.meet(&empty), empty);
    }

    #[test]
    fn partial_order_is_set_inclusion() {
        let small = IntervalSetLattice::from_interval(5u64, 10);
        let big = IntervalSetLattice::from_interval(0u64, 20);
        assert_eq!(small.partial_cmp(&big), Some(Ordering::Less));
        assert_eq!(big.partial_cmp(&small), Some(Ordering::Greater));
        assert_eq!(small.partial_cmp(&small), Some(Ordering::Equal));

        let disjoint = IntervalSetLattice::from_interval(100u64, 200);
        assert_eq!(small.partial_cmp(&disjoint), None);
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn arb_interval_set()(
            ranges in prop::collection::vec((0u64..30, 0u64..30), 0..6)
        ) -> IntervalSetLattice<u64> {
            let mut s = IntervalSetLattice::new();
            for (a, b) in ranges {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                s.insert(lo, hi);
            }
            s
        }
    }

    /// Brute-force the covered point set in `[0, 30)` for cross-checking.
    fn point_set(s: &IntervalSetLattice<u64>) -> alloc::collections::BTreeSet<u64> {
        (0u64..30).filter(|p| s.contains(p)).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(5_000))]

        #[test]
        fn meet_commutative(a in arb_interval_set(), b in arb_interval_set()) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn meet_associative(
            a in arb_interval_set(), b in arb_interval_set(), c in arb_interval_set()
        ) {
            prop_assert_eq!(a.meet(&b.meet(&c)), a.meet(&b).meet(&c));
        }

        #[test]
        fn meet_idempotent(a in arb_interval_set()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn join_commutative(a in arb_interval_set(), b in arb_interval_set()) {
            prop_assert_eq!(a.join(&b), b.join(&a));
        }

        #[test]
        fn join_associative(
            a in arb_interval_set(), b in arb_interval_set(), c in arb_interval_set()
        ) {
            prop_assert_eq!(a.join(&b.join(&c)), a.join(&b).join(&c));
        }

        #[test]
        fn join_idempotent(a in arb_interval_set()) {
            prop_assert_eq!(a.join(&a), a);
        }

        /// meet matches set-intersection of the covered points.
        #[test]
        fn meet_matches_point_intersection(
            a in arb_interval_set(), b in arb_interval_set()
        ) {
            let m = point_set(&a.meet(&b));
            let expected: alloc::collections::BTreeSet<u64> =
                point_set(&a).intersection(&point_set(&b)).copied().collect();
            prop_assert_eq!(m, expected);
        }

        /// join matches set-union of the covered points.
        #[test]
        fn join_matches_point_union(
            a in arb_interval_set(), b in arb_interval_set()
        ) {
            let j = point_set(&a.join(&b));
            let expected: alloc::collections::BTreeSet<u64> =
                point_set(&a).union(&point_set(&b)).copied().collect();
            prop_assert_eq!(j, expected);
        }

        /// The universal consistency law: a ≤ b ⟺ meet(a,b)==a ⟺ join(a,b)==b.
        #[test]
        fn consistency_law(a in arb_interval_set(), b in arb_interval_set()) {
            let le = matches!(a.partial_cmp(&b), Some(o) if o.is_le());
            prop_assert_eq!(le, a.meet(&b) == a);
            prop_assert_eq!(le, a.join(&b) == b);
        }
    }
}
