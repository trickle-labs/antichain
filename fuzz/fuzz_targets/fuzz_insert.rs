//! Fuzz target: `Antichain::insert`
//!
//! Verifies that after any sequence of insertions the antichain invariant holds:
//! no element in the set is `<=` any other element.
//!
//! Run with:
//!   cargo fuzz run fuzz_insert

#![no_main]

use antichain::Antichain;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut ac = Antichain::<u64>::empty();

    // Interpret every 8 bytes as a u64 element to insert.
    for chunk in data.chunks_exact(8) {
        let val = u64::from_le_bytes(chunk.try_into().unwrap());
        ac.insert(val);
    }

    // Assert the invariant: no two elements satisfy x <= y.
    let els = ac.elements();
    for (i, x) in els.iter().enumerate() {
        for (j, y) in els.iter().enumerate() {
            if i != j {
                assert!(
                    !(x <= y),
                    "antichain invariant violated: element[{i}]={x} <= element[{j}]={y}"
                );
            }
        }
    }
});
