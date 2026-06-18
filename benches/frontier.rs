// Phase 3 benchmarks: stress-test antichain width under composition.
//
// Critical path identified in the roadmap: does the antichain explode in size
// for high-dimensional `T`?  These benchmarks provide the data.
//
// Element construction trick: `ProductTimestamp::new(i, WIDTH - i)` for
// `i in 0..WIDTH` yields WIDTH mutually incomparable elements in product order
// because `(i, W-i) ≤ (j, W-j)` iff `i ≤ j AND j ≤ i` iff `i == j`.

use antichain::{Antichain, Frontier, ProductTimestamp};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

// ── Antichain insert — measures O(n) scan per insert, O(n²) total ─────────────

fn bench_antichain_insert_product(c: &mut Criterion) {
    let mut group = c.benchmark_group("antichain_insert_product");
    for width in [10u64, 100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, &w| {
            b.iter(|| {
                let mut a = Antichain::<ProductTimestamp<u64, u64>>::empty();
                for i in 0..w {
                    a.insert(ProductTimestamp::new(i, w - i));
                }
                a
            });
        });
    }
    group.finish();
}

// ── Frontier meet — measures O(n²) merge of two width-n antichains ────────────
//
// f1 elements: (2i,   2W − 2i)   — even-indexed diagonal
// f2 elements: (2i+1, 2W − 2i+1) — odd-indexed diagonal
// f1[i] ≤ f2[i] so all of f2 is eliminated but every insertion still scans
// the full f1 antichain, exercising the quadratic inner loop.

fn bench_frontier_meet_wide(c: &mut Criterion) {
    let mut group = c.benchmark_group("frontier_meet_wide");
    for width in [10u64, 100, 500, 1000] {
        let f1 = Frontier::from_elements(
            (0..width).map(|i| ProductTimestamp::new(2 * i, 2 * width - 2 * i)),
        );
        let f2 = Frontier::from_elements(
            (0..width).map(|i| ProductTimestamp::new(2 * i + 1, 2 * width - 2 * i + 1)),
        );
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter(|| f1.meet(&f2));
        });
    }
    group.finish();
}

// ── Baseline: Frontier<u64> insert chain ─────────────────────────────────────

fn bench_frontier_meet_u64(c: &mut Criterion) {
    let mut group = c.benchmark_group("frontier_meet_u64");
    for width in [10u64, 100, 500, 1000] {
        let f1 = Frontier::from_elements(0..width);
        let f2 = Frontier::from_elements(0..width);
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter(|| f1.meet(&f2));
        });
    }
    group.finish();
}

// ── Antichain::less_equal (dominates check) — O(n) scan ──────────────────────
//
// Measures the hot-path query: "is this timestamp still in-flight?"
// Worst case: the queried time is incomparable with all elements, so every
// element is examined before returning false.

fn bench_antichain_dominates(c: &mut Criterion) {
    let mut group = c.benchmark_group("antichain_dominates");
    for width in [1u64, 10, 100, 500, 1000] {
        // Build a wide antichain of incomparable ProductTimestamp elements.
        let mut a = antichain::Antichain::<ProductTimestamp<u64, u64>>::empty();
        for i in 0..width {
            a.insert(ProductTimestamp::new(i, width - i));
        }
        // Query an element that is incomparable with all (falls off the anti-diagonal).
        let probe = ProductTimestamp::new(width + 1, width + 1);
        group.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter(|| a.less_equal(&probe));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_antichain_insert_product,
    bench_frontier_meet_wide,
    bench_frontier_meet_u64,
    bench_antichain_dominates,
);
criterion_main!(benches);
