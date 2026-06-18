# From One Number to a Frontier

A narrative introduction to coordinator-free progress tracking.

---

## The problem: you need a global watermark

Suppose you are building a stream-processing system. Five workers are chewing through
a Kafka topic in parallel. Somewhere downstream, an aggregation step wants to know:

> *"Is it safe to finalize the window for minute 15:00? Has everyone gotten past 15:00?"*

The answer is "yes" only when **every** worker is past timestamp 900 000 (seconds → ms or
whatever unit you use). You need a **watermark**: a guarantee that no worker will ever
produce a record at or below that point again.

---

## Step 1: the naive coordinator

The first instinct is a coordinator. Give one node the job of collecting reports and
computing the minimum:

```rust
/// A dumb global-min coordinator.
struct Coordinator {
    worker_progress: std::collections::HashMap<u32, u64>,
}

impl Coordinator {
    fn new() -> Self {
        Self { worker_progress: std::collections::HashMap::new() }
    }

    fn report(&mut self, worker_id: u32, progress: u64) {
        self.worker_progress.insert(worker_id, progress);
    }

    fn watermark(&self) -> Option<u64> {
        self.worker_progress.values().copied().reduce(u64::min)
    }
}

let mut coord = Coordinator::new();
coord.report(0, 900_050);
coord.report(1, 900_010);
coord.report(2, 900_200);

assert_eq!(coord.watermark(), Some(900_010));
```

This works — but every read and write goes through one place. The coordinator is now:

- A **bottleneck**: all workers must synchronize through it.
- A **single point of failure**: if it crashes, the watermark is unavailable.
- A **consistency hazard**: partial updates leave the watermark transiently wrong.

---

## Step 2: can two workers merge without a coordinator?

Let's back up. What does "merge two progress reports" actually mean? Worker A is at
timestamp 900 050. Worker B is at timestamp 900 010. The *safe* answer — the most
conservative bound — is `min(900_050, 900_010) = 900_010`.

What if we just… compute that directly?

```rust
use antichain::Frontier;

let a = Frontier::from_elem(900_050u64);
let b = Frontier::from_elem(900_010u64);

// meet = greatest lower bound = min for totally-ordered types
let merged = a.meet(&b);
assert_eq!(merged, Frontier::from_elem(900_010u64));
```

That's it. `Frontier::meet` computes the most conservative progress bound. No coordinator
involved.

But does it work if we apply the merges in a different order? What if A merges B before
B knows about A?

```rust
use antichain::Frontier;

let a = Frontier::from_elem(900_050u64);
let b = Frontier::from_elem(900_010u64);
let c = Frontier::from_elem(900_200u64);

// Three workers, three different merge orderings:
let order1 = a.meet(&b).meet(&c);
let order2 = c.meet(&a).meet(&b);
let order3 = b.meet(&c).meet(&a);

// All three produce the same answer.
assert_eq!(order1, order2);
assert_eq!(order2, order3);
```

This works because `meet` is:

- **Commutative** — `meet(a, b) == meet(b, a)`: order of operands does not matter.
- **Associative** — `meet(a, meet(b, c)) == meet(meet(a, b), c)`: grouping does not matter.
- **Idempotent** — `meet(a, a) == a`: duplicates are harmless.

These three properties together mean that workers can gossip progress over any network —
reordered, delayed, duplicated — and still converge to the identical answer. **The
coordinator is no longer necessary.**

---

## Step 3: what is a `Frontier`, really?

A `Frontier<T>` is a progress *claim*: *"all timestamps strictly below this boundary
are complete."* You can think of it as a watermark line.

```rust
use antichain::Frontier;

let f = Frontier::from_elem(900_010u64);

// less_equal(t) returns true when t <= some frontier element.
// The frontier is at 900_010, so timestamps at or below it are "within" the covered range.
assert!(f.less_equal(&900_009));  // 900_009 <= 900_010 → within the frontier
assert!(f.less_equal(&900_010));  // 900_010 <= 900_010 → at the frontier boundary

// Returns false when t is strictly above the frontier element:
assert!(!f.less_equal(&901_000)); // 901_000 > 900_010 → above the frontier
```

`less_equal` answers: *"Has the frontier advanced to cover this timestamp?"* `true` means
the frontier has reached or passed `t`; `false` means the frontier has not yet gotten to `t`.

---

## Step 4: the antichain invariant

So far, every frontier has been a single number (`width = 1`). That works for
totally-ordered types like `u64`. But what if your timestamps are **multi-dimensional**?

Imagine progress along two independent axes: partition ID and byte offset. Worker A has
processed partition 0 up to offset 500, but hasn't touched partition 1. Worker B has
processed partition 1 up to offset 300, but is stalled on partition 0. Neither dominates
the other — they are *incomparable*.

The frontier is now a **set** of incomparable elements. This set is the *antichain*:
no element in the set is less-than-or-equal to any other.

```rust
use antichain::{Antichain, ProductTimestamp};

// Two incomparable progress points:
// partition 0, offset 500
// partition 1, offset 300
let mut ac: Antichain<ProductTimestamp<u64, u64>> = Antichain::empty();
ac.insert(ProductTimestamp::new(0, 500));
ac.insert(ProductTimestamp::new(1, 300));

// Both are in the antichain because neither dominates the other.
assert_eq!(ac.len(), 2);
```

The antichain invariant is maintained automatically by `insert`: if you insert an element
that is less-than-or-equal to an existing element, the insert is a no-op. If you insert
one that dominates an existing element, the old one is removed.

```rust
use antichain::{Antichain, ProductTimestamp};

let mut ac: Antichain<ProductTimestamp<u64, u64>> = Antichain::empty();
ac.insert(ProductTimestamp::new(0, 500));
ac.insert(ProductTimestamp::new(1, 300));

// Inserting a smaller element that dominates all existing ones collapses the antichain.
// (0, 100) <= (0, 500) because 0<=0 and 100<=500 → removes (0, 500).
// (0, 100) <= (1, 300) because 0<=1 and 100<=300 → removes (1, 300).
ac.insert(ProductTimestamp::new(0, 100));
assert_eq!(ac.len(), 1);
```

---

## Step 5: multi-dimensional frontiers

Wrapping the antichain in a `Frontier` gives you the same coordinator-free merge, but
now across multi-dimensional timestamps:

```rust
use antichain::{Frontier, ProductTimestamp};

// Three workers with two-dimensional progress (partition, offset):
let worker_a = Frontier::from_elem(ProductTimestamp::new(0u64, 500u64));
let worker_b = Frontier::from_elem(ProductTimestamp::new(1u64, 300u64));
let worker_c = Frontier::from_elem(ProductTimestamp::new(0u64, 400u64));

// meet collapses to the most conservative bound across every dimension.
// For ProductTimestamp, meet is component-wise: (min(outer), min(inner)).
// meet(A, B): outer=min(0,1)=0, inner=min(500,300)=300 → (0, 300)
// But A and B are incomparable, so the frontier keeps both elements.
let ab = worker_a.meet(&worker_b);

// The merged frontier has two incomparable elements.
assert_eq!(ab.elements().len(), 2);

// A timestamp is in-flight if it is less_equal ANY element of the antichain.
// (0, 250) ≤ (0, 300) under product order → in-flight.
assert!(ab.less_equal(&ProductTimestamp::new(0u64, 250u64)));
```

The antichain handles the width automatically. For totally-ordered types like `u64`, it
always collapses to width 1 (allocation-free). For partially-ordered types, it holds as
many incomparable elements as needed.

---

## Step 6: putting it together

Here is the original five-worker watermark scenario, refactored to use `Frontier`:

```rust
use antichain::Frontier;

// Five workers report their progress independently.
let workers: Vec<Frontier<u64>> = vec![
    Frontier::from_elem(900_050u64),
    Frontier::from_elem(900_010u64),
    Frontier::from_elem(900_200u64),
    Frontier::from_elem(901_000u64),
    Frontier::from_elem(900_005u64),
];

// Merge them all — order doesn't matter, duplicates are fine.
let global = workers.iter().fold(workers[0].clone(), |acc, w| acc.meet(w));

// The global frontier is the most conservative bound across all workers.
assert_eq!(global, Frontier::from_elem(900_005u64));

// less_equal(t) = t <= frontier_element.
// Timestamps at or below the frontier are "covered"; above it are not yet reached.
assert!(global.less_equal(&900_005));   // 900_005 <= 900_005 → at the frontier boundary
assert!(global.less_equal(&900_000));   // 900_000 <= 900_005 → within the frontier
assert!(!global.less_equal(&900_100));  // 900_100 > 900_005 → above the frontier
```

No coordinator. No lock. No single point of failure. Any subset of workers can gossip
their `Frontier` values to each other and always converge.

---

## Where to go next

- **[Cookbook](cookbook.md)** — decision table and worked recipes for every type.
- **[API reference](https://docs.rs/antichain)** — full type documentation.
- **[Runnable examples](../examples/)** — `watermark_gossip.rs` runs a simulated N-node
  gossip loop; `backfill_gaps.rs` demonstrates out-of-order progress tracking.
- **[Design notes](idea.md)** — the algebra and the boundaries of the problem.
