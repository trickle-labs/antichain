//! Watermark gossip — a runnable convergence demonstration.
//!
//! Simulates N workers exchanging frontiers over a lossy, reordering in-memory
//! "network." Each worker advances its own progress independently and periodically
//! gossips its current `Frontier` to a random peer. The simulation demonstrates the
//! **convergence theorem**: regardless of the gossip order or which messages are
//! dropped, all workers converge to the same global watermark once they have
//! collectively seen all updates.
//!
//! # What to watch for
//!
//! Each round prints every worker's current frontier and the global minimum (what a
//! downstream consumer would safely see). Watch how quickly the global min stabilises
//! even when 30% of gossip messages are dropped. By the final round every worker holds
//! the same value — convergence without any coordinator.
//!
//! # Run
//!
//! ```sh
//! cargo run --example watermark_gossip
//! ```

use antichain::Frontier;

fn main() {
    const N_WORKERS: usize = 6;
    const N_ROUNDS: usize = 8;
    // Deterministic "randomness" — lcg-style, so output is reproducible.
    let mut rng = Lcg::new(0xdeadbeef);

    // Each worker starts at a different offset.
    let initial_offsets: [u64; N_WORKERS] = [42, 17, 55, 31, 48, 9];
    let mut frontiers: Vec<Frontier<u64>> = initial_offsets
        .iter()
        .map(|&t| Frontier::from_elem(t))
        .collect();

    println!("=== Watermark gossip: {N_WORKERS} workers, {N_ROUNDS} rounds ===\n");
    println!("Initial offsets: {initial_offsets:?}\n");

    for round in 1..=N_ROUNDS {
        // Each worker advances its own progress by a random increment.
        for i in 0..N_WORKERS {
            let advance = rng.next() % 12 + 1; // 1..=12
            let current = *frontiers[i]
                .elements()
                .first()
                .expect("frontier is non-empty");
            frontiers[i] = Frontier::from_elem(current + advance);
        }

        // Gossip phase: each worker picks a random peer; 30% of messages are dropped.
        let gossip_count = N_WORKERS * 2;
        let mut delivered = 0usize;
        let mut dropped = 0usize;
        for _ in 0..gossip_count {
            let sender = (rng.next() as usize) % N_WORKERS;
            let receiver = (rng.next() as usize) % N_WORKERS;
            if receiver == sender {
                continue; // skip self-gossip
            }
            // Simulate 30% packet loss.
            if rng.next() % 10 < 3 {
                dropped += 1;
                continue;
            }
            // Receiver merges sender's frontier into its own.
            let sender_view = frontiers[sender].clone();
            frontiers[receiver] = frontiers[receiver].meet(&sender_view);
            delivered += 1;
        }

        // Compute the ground-truth global minimum across all current worker offsets.
        let global_min = frontiers
            .iter()
            .map(|f| *f.elements().first().expect("non-empty"))
            .min()
            .expect("at least one worker");

        // Print round summary.
        let values: Vec<u64> = frontiers
            .iter()
            .map(|f| *f.elements().first().expect("non-empty"))
            .collect();
        println!(
            "Round {round:>2}: workers={values:?}  global_min={global_min}  \
             (gossip: {delivered} delivered, {dropped} dropped)"
        );
    }

    // Final convergence check: all workers should share the same frontier after
    // a final unconditional all-to-all gossip round.
    println!("\n--- Final all-to-all gossip (convergence check) ---");
    let snapshot: Vec<Frontier<u64>> = frontiers.clone();
    for i in 0..N_WORKERS {
        for j in 0..N_WORKERS {
            if i != j {
                let peer = snapshot[j].clone();
                frontiers[i] = frontiers[i].meet(&peer);
            }
        }
    }

    let converged_values: Vec<u64> = frontiers
        .iter()
        .map(|f| *f.elements().first().expect("non-empty"))
        .collect();
    println!("All workers: {converged_values:?}");

    let first = converged_values[0];
    let all_equal = converged_values.iter().all(|&v| v == first);
    if all_equal {
        println!("✓ Convergence: all {N_WORKERS} workers hold frontier = {first}");
    } else {
        eprintln!("✗ Convergence FAILED — workers diverged!");
        std::process::exit(1);
    }
}

// ── Minimal deterministic LCG pseudo-random number generator ─────────────────

/// A tiny linear-congruential generator — no external dependencies, reproducible output.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns the next pseudo-random u64.
    fn next(&mut self) -> u64 {
        // Knuth's multiplicative LCG constants (64-bit variant).
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
}
