//! Backfill with gaps — tracking out-of-order progress using `IntervalSetLattice`.
//!
//! A backfill engine re-processes historical blocks from a distributed log. Blocks
//! arrive out of order (network reordering, different-speed workers), so progress
//! is not a simple monotonic offset — it is a *set of covered ranges with holes*.
//!
//! `IntervalSetLattice<T>` (from the `antichain-intervals` companion crate) models
//! exactly this: a canonical set of disjoint half-open intervals `[start, end)`.
//!
//! - **`join`** (union with coalescing) — what either worker has covered so far.
//! - **`meet`** (intersection) — what *both* workers have safely covered.
//!
//! The safe-to-acknowledge range at any moment is the `meet` of all workers'
//! interval sets: it only includes ranges that *every* worker has confirmed.
//!
//! # What to watch for
//!
//! Watch the "safe ack" column. A gap in one worker's coverage blocks the safe
//! acknowledgement even when others have advanced far beyond it — until that
//! worker fills the hole. Once every worker has covered the same contiguous range,
//! the safe coverage snaps forward all at once.
//!
//! # Run
//!
//! ```sh
//! cargo run --example backfill_gaps --features antichain-intervals/std
//! ```
//!
//! Or, since the workspace already pulls in the crate:
//!
//! ```sh
//! cargo run --example backfill_gaps
//! ```

use antichain::Lattice;
use antichain_intervals::IntervalSetLattice;

fn main() {
    println!("=== Backfill-with-gaps convergence demo ===\n");
    println!("Three workers re-process blocks [0, 1000) from a distributed log.");
    println!("Blocks arrive out of order. Gaps block safe acknowledgement.\n");

    // ── Simulate three workers processing different block ranges independently ──

    // Worker 0: fast, starts from the beginning, hits a hole at 300–350.
    let mut w0 = IntervalSetLattice::new();
    w0.insert(0u64, 300);    // processed blocks 0–299
    w0.insert(350, 700);     // then jumped ahead; gap at 300–349

    // Worker 1: slow start, but no gaps.
    let mut w1 = IntervalSetLattice::new();
    w1.insert(0u64, 600);    // processed blocks 0–599

    // Worker 2: started in the middle, wrapping back to the start later.
    let mut w2 = IntervalSetLattice::new();
    w2.insert(200u64, 800);  // processed blocks 200–799

    print_state("Initial state", &w0, &w1, &w2);

    // ── Round 1: what can we safely acknowledge? ──────────────────────────────
    // meet = intersection = what EVERY worker has covered.
    let safe_r1 = w0.meet(&w1).meet(&w2);
    println!("Round 1 — safe to ack (meet of all workers):");
    println!("  {:?}", safe_r1.intervals());
    println!("  → Only ranges covered by all three workers: [200,300) and [350,600).\n");

    // ── Round 2: worker 0 fills the gap ──────────────────────────────────────
    println!("--- Worker 0 fills gap [300, 350) ---\n");
    w0.insert(300u64, 350);

    // join = union = what we now collectively know
    let union_r2 = w0.join(&w1).join(&w2);
    println!("Union coverage (optimistic, any worker):");
    println!("  {:?}", union_r2.intervals());

    let safe_r2 = w0.meet(&w1).meet(&w2);
    println!("Safe to ack (meet of all workers):");
    println!("  {:?}", safe_r2.intervals());
    println!("  → [200, 600) is now confirmed by all three workers.\n");

    // ── Round 3: workers finish their respective ranges ───────────────────────
    println!("--- All workers cover [0, 1000) ---\n");
    w0.insert(700u64, 1000);   // w0 now covers [0,300)+[300,350)+[350,700)+[700,1000) = [0,1000)
    w1.insert(600u64, 1000);   // w1 now covers [0,1000)
    w2.insert(0u64, 200);      // w2 fills [0,200), join with [200,800)
    w2.insert(800u64, 1000);   // w2 finishes

    print_state("Final state", &w0, &w1, &w2);

    let safe_final = w0.meet(&w1).meet(&w2);
    println!("Final safe to ack (meet of all workers):");
    println!("  {:?}", safe_final.intervals());

    let expected = [(0u64, 1000u64)];
    if safe_final.intervals() == expected {
        println!("\n✓ All blocks [0, 1000) are safely acknowledged by all workers.");
    } else {
        eprintln!("\n✗ Unexpected coverage: {:?}", safe_final.intervals());
        std::process::exit(1);
    }

    // ── Demonstrate `IntervalSetLattice` inside a `Frontier` ─────────────────
    println!("\n--- Using IntervalSetLattice as a Frontier value type ---\n");

    use antichain::Frontier;

    // Each shard tracks its own interval coverage. The Frontier wraps the antichain
    // over these sets — here each shard has a single interval-set progress point.
    let shard_0_progress: IntervalSetLattice<u64> = {
        let mut s = IntervalSetLattice::new();
        s.insert(0u64, 500);
        s
    };
    let shard_1_progress: IntervalSetLattice<u64> = {
        let mut s = IntervalSetLattice::new();
        s.insert(0u64, 750);
        s
    };

    let f0 = Frontier::from_elem(shard_0_progress);
    let f1 = Frontier::from_elem(shard_1_progress);

    let global = f0.meet(&f1);
    // The Frontier<IntervalSetLattice> collapses to width 1 (totally ordered for this input).
    let safe_intervals = global.elements();
    println!("Global frontier element count: {}", safe_intervals.len());
    println!("Safe intervals: {:?}", safe_intervals[0].intervals());
    println!("✓ Frontier<IntervalSetLattice> composes correctly.");
}

fn print_state(
    label: &str,
    w0: &IntervalSetLattice<u64>,
    w1: &IntervalSetLattice<u64>,
    w2: &IntervalSetLattice<u64>,
) {
    println!("{label}:");
    println!("  Worker 0: {:?}", w0.intervals());
    println!("  Worker 1: {:?}", w1.intervals());
    println!("  Worker 2: {:?}", w2.intervals());
    println!();
}
