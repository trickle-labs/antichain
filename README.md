# antichain

A coordinator-free primitive for tracking distributed progress using lattice algebra.

> **Scope:** progress tracking only. No ownership, no membership, no consensus.

[![CI](https://github.com/geir-gronmo/antichain/actions/workflows/ci.yml/badge.svg)](https://github.com/geir-gronmo/antichain/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/antichain.svg)](https://crates.io/crates/antichain)
[![docs.rs](https://docs.rs/antichain/badge.svg)](https://docs.rs/antichain)

---

## The core idea

In large distributed systems, nodes must repeatedly answer:

> *"Is it safe to commit / emit a result / advance now?"*

The usual answer is a single global sequence number — a watermark, an epoch — funneled through a
central coordinator. This works, but the coordinator is a structural bottleneck: all progress
must be expressed as one integer on one line, owned by one party.

**Antichain** attacks the modeling choice, not the coordinator's hardware.

A `Frontier<T>` represents progress as an **antichain of timestamps** — a set of mutually
incomparable points on the boundary of what is complete. The key operation, `meet`, is:

- **Commutative** — `meet(a, b) == meet(b, a)`
- **Associative** — `meet(a, meet(b, c)) == meet(meet(a, b), c)`
- **Idempotent** — `meet(a, a) == a`

These three laws mean nodes can exchange progress states in any order, over any network, with
duplicates or reordering, and still converge to the identical correct answer — with no lock, no
pause, and no leader.

## What this crate is

- `Lattice` — a trait for types with `meet` (GLB) and `join` (LUB) operations
- `Antichain<T>` — a set of mutually incomparable elements of `T`, maintained automatically
- `Frontier<T>` — a progress claim backed by an `Antichain<T>`
- `ProductTimestamp<T1, T2>` — multi-dimensional time as a product partial order

## What this crate is not

- A networking layer or gossip protocol
- A consensus or lease mechanism
- A storage engine

Those are things you might *build on* this primitive. They are not the primitive.

## Usage

```toml
[dependencies]
antichain = "0.1"
# with serde support:
# antichain = { version = "0.1", features = ["serde"] }
```

```rust
use antichain::Frontier;

// Two workers report their progress independently.
let worker_a = Frontier::from_elem(10u64);
let worker_b = Frontier::from_elem(7u64);

// The global frontier is the meet — the most conservative bound.
let global = worker_a.meet(&worker_b);
assert_eq!(global, Frontier::from_elem(7u64));

// Order of operations doesn't matter.
assert_eq!(worker_a.meet(&worker_b), worker_b.meet(&worker_a));
```

## Design notes

See [`docs/idea.md`](docs/idea.md) for the full motivation, the algebraic reasoning, and the
boundaries of the problem this crate solves.

## License

MIT
