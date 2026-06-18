//! Fuzz target: `Frontier::meet`
//!
//! Verifies the core properties of `meet` (coordinator-free merge) under arbitrary inputs:
//! - The antichain invariant is preserved in the result.
//! - meet is idempotent: `meet(a, a) == a`.
//! - meet is commutative: `meet(a, b) == meet(b, a)`.
//!
//! Run with:
//!   cargo fuzz run fuzz_meet

#![no_main]

use antichain::{Antichain, Frontier};
use libfuzzer_sys::fuzz_target;

/// Build a `Frontier<u64>` from a byte slice: each 8-byte chunk is one u64 element.
fn frontier_from_bytes(data: &[u8]) -> Frontier<u64> {
    Frontier::from_elements(
        data.chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap())),
    )
}

/// Assert the antichain invariant holds on a frontier's elements.
fn assert_invariant(f: &Frontier<u64>, label: &str) {
    let els = f.elements();
    for (i, x) in els.iter().enumerate() {
        for (j, y) in els.iter().enumerate() {
            if i != j {
                assert!(
                    !(x <= y),
                    "{label}: antichain invariant violated: element[{i}]={x} <= element[{j}]={y}"
                );
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    // Use the first byte as a split point to create two independent byte slices.
    let split = (data[0] as usize).min(data.len() - 1);
    let (raw_a, raw_b) = data[1..].split_at(split.min(data.len() - 1));

    let a = frontier_from_bytes(raw_a);
    let b = frontier_from_bytes(raw_b);

    let ab = a.meet(&b);
    let ba = b.meet(&a);

    // Invariant preserved.
    assert_invariant(&ab, "meet(a,b)");
    assert_invariant(&ba, "meet(b,a)");

    // Commutativity.
    assert_eq!(ab, ba, "meet is not commutative");

    // Idempotence.
    assert_eq!(a.meet(&a), a, "meet(a,a) != a");
    assert_eq!(b.meet(&b), b, "meet(b,b) != b");

    // Absorption: meet(meet(a,b), a) == meet(a,b)
    assert_eq!(ab.meet(&a), ab, "absorption: meet(meet(a,b), a) != meet(a,b)");
    assert_eq!(ab.meet(&b), ab, "absorption: meet(meet(a,b), b) != meet(a,b)");
});
