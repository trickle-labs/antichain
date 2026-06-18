# Antichain — Frequently Asked Questions

A friendly, plain-language guide to what `antichain` is, why it exists, and how to use
it. It starts gently — no maths background assumed — and gradually goes deeper for
software engineers and the mathematically curious.

> **New here?** Read the first two sections ([The big picture](#the-big-picture) and
> [Core ideas in plain words](#core-ideas-in-plain-words)) and you will understand what
> this crate is for. Everything after that is optional depth.
>
> Prefer a worked story? Start with the **[Tutorial](tutorial.md)**. Want a
> "which-type-for-which-problem" lookup? Use the **[Cookbook](cookbook.md)**.

---

## Table of contents

1. [The big picture](#the-big-picture) — *for everyone*
2. [Core ideas in plain words](#core-ideas-in-plain-words) — *for everyone*
3. [The maths, gently](#the-maths-gently) — *for the curious*
4. [For software engineers: using the library](#for-software-engineers-using-the-library)
5. [Choosing and composing types](#choosing-and-composing-types)
6. [Performance and internals](#performance-and-internals)
7. [Correctness, testing, and formal proofs](#correctness-testing-and-formal-proofs)
8. [How it compares to other tools](#how-it-compares-to-other-tools)
9. [Project, packaging, and practical matters](#project-packaging-and-practical-matters)
10. [Troubleshooting and common gotchas](#troubleshooting-and-common-gotchas)

---

## The big picture

### 1. What is `antichain` in one sentence?

It is a small Rust library for tracking *how far along* a job is across many computers — and
for combining all those separate "I've reached here" reports into one trustworthy answer
without needing a central "boss" computer to keep score. Think of it as the tiny, rock-solid
piece of maths that sits underneath a distributed system and answers the question *"how far
has everyone collectively gotten?"* no matter how messy the network is underneath.

### 2. What problem does it actually solve?

Imagine a hundred workers chewing through a giant pile of work — events to process, files to
ingest, rows to replicate. Every so often something downstream needs to ask: *"Is it safe to
act now? Has everyone gotten far enough that I won't miss anything?"* For example, you only
want to publish the hourly report once *every* worker has finished processing that hour's
events. The obvious design is to nominate one machine to collect everyone's progress and
announce the global answer. `antichain` gives you a different option: the workers combine
their progress reports **directly with each other**, using one simple merge operation, and
the correct global answer falls out automatically. No machine has to be "in charge."

### 3. Why is avoiding a central coordinator a big deal?

A central coordinator quietly creates three separate headaches. It is a **bottleneck** —
every progress report in the system funnels through one machine, so it caps how fast you can
go. It is a **single point of failure** — if it crashes or gets partitioned away, nobody can
learn the global progress until it comes back. And it is a **consistency hazard** — while it
is half-way through ingesting a batch of updates, it can hand out an answer that is briefly
wrong. Because `antichain`'s merge is order-independent and repeat-safe, you can delete the
coordinator entirely and all three problems vanish at once: there's nothing to overload,
nothing whose death is fatal, and no "half-updated" intermediate state to leak.

### 4. Can you give me a real-world analogy?

Think of a group of hikers strung out along a long trail. You want to know *"where is the
slowest hiker, so we know everyone is at least that far along?"* The centralized way: every
hiker radios a designated leader who keeps track of the minimum and broadcasts it back. The
`antichain` way: whenever any two hikers happen to meet on the trail, they compare notes and
each remembers the more-conservative (further-back) position of the two. That's it. After
enough chance encounters, *every* hiker independently knows the true position of the slowest
person — with no leader, no radio tower, and no dependence on who bumped into whom or in what
order. Even if two hikers meet twice, or pass along stale information, the answer everyone
converges to is identical and correct. That "remember the more conservative value when you
meet" step is exactly what `meet` does.

### 5. Who is this library for?

It is for people building distributed or parallel systems where progress is spread across
many workers: stream processors that need watermarks, databases coordinating replication,
backfill and ingestion pipelines that pull data out of order, change-data-capture systems,
and anything that repeatedly needs to answer *"has everyone passed point X yet?"* If you have
ever written code that gathers "how far did you get?" numbers from a fleet of workers and
takes the minimum, you are this library's audience — it makes that pattern correct,
composable, and coordinator-free.

### 6. Do I need to be a mathematician to use it?

No — and that's deliberate. The day-to-day workflow is just "pick the type that matches the
shape of your progress, then call `meet` to combine reports." The lattice theory underneath
exists to *guarantee* that this always gives the right answer, but you never have to derive
or even read it to get the benefit, much like you don't need to understand TCP's congestion
proofs to open a socket. If you want the intuition anyway, the [Tutorial](tutorial.md) builds
it up gently from a single number, and the [Cookbook](cookbook.md) is a plain problem-to-type
lookup written for working engineers.

### 7. Is this a database, a message queue, or a networking library?

None of those — and keeping it that way is the whole point. `antichain` is a **pure data
type**: a building block with no networking, no disk I/O, no threads, and no background tasks.
You feed it progress values, and it tells you how they combine. Everything around it — how
you ship reports between machines, where you persist them, how often you gossip — is yours to
build however suits your system. This is a feature, not a limitation: because the primitive
does one thing and makes no assumptions about your transport, it drops cleanly into a Kafka
consumer, a gRPC service, an embedded device, or a single-process thread pool alike.

### 8. What does the name "antichain" mean?

The name comes from order theory. A **chain** is a set of things you can line up neatly from
least to greatest (like the numbers 1, 2, 3). An **antichain** is the opposite: a set of
things where no item is "ahead of" or "behind" any other — they are all mutually
incomparable. That sounds abstract, but it is exactly what you need to honestly describe
multi-dimensional progress. If worker A has finished partition 0 but not 1, and worker B has
finished partition 1 but not 0, neither is "further along" overall — so the honest boundary
of completed work is the *antichain* containing both. (See
[§24](#24-what-is-an-antichain-precisely) for the precise definition.)

### 9. Is it production-ready?

The core is small, fully property-tested over tens of thousands of random cases, formally
model-checked for its central convergence guarantee, benchmarked on real hardware, and
published on crates.io. It is intentionally "one-idea-sharp": rather than a sprawling
framework, it is a tiny primitive that does one thing and is proven to do it correctly. As
with any `0.x` crate the public API can still evolve, so the practical advice is the usual
one — read the [CHANGELOG](../CHANGELOG.md) and pin a version you've tested against.

### 10. What's the quickest way to see it work?

Two lines capture the whole idea — combining two progress claims keeps the more conservative
one:

```rust
use antichain::Frontier;
let global = Frontier::from_elem(10u64).meet(&Frontier::from_elem(7u64));
assert_eq!(global, Frontier::from_elem(7u64)); // the more conservative bound wins
```

Here one worker has finished everything up to `10` and another only up to `7`; the only thing
safely true for *both* is "done up to `7`," so that's what `meet` returns. To watch the same
idea play out across many simulated workers gossiping over a lossy network, run the live
demo: `cargo run --example watermark_gossip`.

---

## Core ideas in plain words

### 11. What is a "frontier"?

A **frontier** is a progress claim — a line in the sand that says *"everything below this
line is finished."* If a worker reports a frontier of `7`, it is promising *"I've completed
everything up to 7; anything at or beyond 7 might still be in flight."* This is precisely the
same concept that stream processors call a **watermark**: a moving boundary that separates
"definitely done" from "maybe not yet." The power of treating it as a first-class value is
that you can store it, ship it across the network, and — crucially — *combine* it with other
workers' frontiers to learn the global picture.

### 12. What is "merging" two frontiers?

Merging means folding two separate progress claims into the single claim that is safely true
for *both* of them at once. Suppose worker A reports it is done up to `10` and worker B
reports it is done up to `7`. What can you safely promise about the pair? Only "done up to
`7`" — because anything past `7` might still be unfinished on B's side. That combine step,
which always keeps the conservative common ground, is called **`meet`**, and it is the
operation you will reach for constantly.

### 13. What is `meet`?

`meet` is the heartbeat of the whole crate: the **coordinator-free merge**. For plain numbers
it simply computes the *minimum* — the most conservative answer that is safe for everyone. For
richer, multi-dimensional types it computes the equivalent notion, the "greatest lower
bound," which is the closest thing to a minimum that still makes sense when two states can't
be ranked against each other. Whenever you want to know *"what progress can I rely on across
all these workers?"*, you call `meet`:

```rust
use antichain::Frontier;
let safe = Frontier::from_elem(42u64).meet(&Frontier::from_elem(37u64));
assert_eq!(safe, Frontier::from_elem(37u64)); // can't trust anything past 37
```

### 14. What is `join`, then?

`join` is `meet`'s mirror image: the *least upper bound*, the most **optimistic** combination
that captures "the furthest anyone has reached." For plain numbers it is just the *maximum*.
The two have complementary jobs: you use `join` to **advance** your own knowledge as new
reports arrive ("someone got to 50, so my best-known progress is now at least 50"), and you
use `meet` to find the **safe shared** progress you can actually act on ("but the slowest
worker is only at 37, so that's the line I can publish behind").

### 15. Why are `meet` and `join` weird names? Why not `min`/`max`?

Because `min` and `max` only make sense when *every* pair of values can be ranked, and this
library deliberately works for kinds of progress where that isn't true. What is the "minimum"
of the two states "finished partition 0" and "finished partition 1"? There isn't one — they're
incomparable. But the "greatest lower bound" (`meet`) is always well-defined: it's "finished
neither partition yet," the richest state that is still behind both. The names come from
**lattice theory**, where `meet` and `join` have a single precise meaning that holds across
numbers, pairs, sets, maps, and every composite you can build from them.

### 16. What makes the merge safe to do in any order?

Three algebraic properties of `meet`, which together are the secret sauce:

- **Commutative** — `meet(a, b) == meet(b, a)`: the order of the two inputs doesn't matter.
- **Associative** — `meet(a, meet(b, c)) == meet(meet(a, b), c)`: how you group them doesn't
  matter.
- **Idempotent** — `meet(a, a) == a`: merging the same value twice changes nothing.

Because of these three laws, progress messages can arrive **late, out of order, or
duplicated**, and every node still grinds down to the identical, correct answer. There is no
"right" sequence to process updates in — every sequence lands in the same place.

### 17. Why does idempotence matter so much in practice?

Real networks redeliver messages all the time — retries, gossip fan-out, and at-least-once
queues all mean you *will* see the same progress report more than once. If merging a duplicate
could nudge your answer, you'd be forced into exactly-once delivery, which is famously hard
and expensive to build. Idempotence makes that whole class of problem disappear: re-merging a
value you've already seen is a guaranteed no-op, so duplicates are simply harmless. That frees
you to use cheap, robust transports — fire-and-forget gossip, aggressive retries, overlapping
broadcasts — without ever tracking what you've already applied.

### 18. What is "convergence"?

Convergence is the central promise that makes the coordinator deletable: **any two nodes that
have seen the same set of updates — in any order, with any duplicates — end up holding exactly
the same frontier.** It doesn't matter that node X heard the reports as A, B, C while node Y
heard them as C, C, A, B; once they've both absorbed the same underlying facts, their answers
are bit-for-bit identical. That is the formal reason you can trust a decentralized answer as
much as a centralized one. (See [§52](#52-is-the-convergence-guarantee-actually-proven) for
the proof.)

### 19. Is this the same idea as CRDTs?

Yes — it's the same underlying algebra. A CRDT (Conflict-free Replicated Data Type) merges
replicated **data** without coordination so that replicas converge; `antichain` merges
**progress** without coordination so that workers agree on how far the computation has gotten.
Both rely on the same mathematical trick — a commutative, associative, idempotent semilattice
merge — just pointed at different targets. If you already trust CRDTs to keep replicas
consistent, you can trust `antichain` to keep progress tracking consistent for exactly the
same reasons. (See [§59](#59-how-is-this-different-from-a-crdt-library).)

### 20. What's the difference between `Antichain`, `Frontier`, and `Lattice`?

These three names show up everywhere, so it's worth pinning them down:

- **`Lattice`** is a *trait* — an interface that says "this type knows how to `meet` and
  `join`." Any type implementing it can be combined coordinator-free.
- **`Antichain<T>`** is a *set* of mutually-incomparable values that is automatically kept
  minimal: insert something dominated and it's dropped; insert something dominating and the
  old values are removed.
- **`Frontier<T>`** is a *progress claim* backed by an `Antichain<T>`. It's the ergonomic type
  you'll usually hold, ship, and merge in application code.

In short: `Lattice` is the capability, `Antichain` is the invariant-maintaining container, and
`Frontier` is the friendly progress wrapper you reach for first.

### 21. When would a frontier hold more than one value?

Whenever progress is genuinely multi-dimensional and two states are *incomparable* — neither
is strictly ahead of the other. The classic example is partitioned work: worker A has finished
partition 0 but not partition 1, while worker B has finished partition 1 but not partition 0.
Neither dominates, so collapsing them to a single value would be a lie. The frontier instead
keeps **both** points as the honest description of "the boundary of what's complete." Once a
third report dominates one of them (say, someone finishes both partition 0 and 1), the
dominated point is automatically dropped and the frontier shrinks again.

### 22. If I only ever track a single number per worker, is this overkill?

Not at all — that's the simplest, fastest, and most common case, and the library is tuned for
it. A `Frontier<u64>` collapses to a single value, merges in constant time, and never touches
the heap, so you pay essentially zero overhead. You still get every coordinator-free guarantee
— order-independence, duplicate-safety, convergence — wrapped around what is effectively a
`min` operation. The multi-dimensional machinery is there when you need it and entirely out of
the way when you don't.

### 23. What does "coordinator-free" *not* mean?

It is not a claim that you need no networking, and it is not a promise of magically consistent
application data. You still have to *ship* the progress values between nodes somehow — over
gossip, a message bus, RPC, whatever you prefer. What "coordinator-free" buys you is that
*however* those values travel — any order, any duplication, any delay — the merged result is
always the correct one, with no central referee needed to impose an order. The transport is
yours; the correctness of the combine is the library's.

---

## The maths, gently

*You can use the whole library without this section. It's here for the curious and for
people who want to understand* why *the guarantees hold.*

### 24. What is an antichain, precisely?

Start with a partial order — a notion of "≤" that not every pair of elements has to satisfy.
An **antichain** is a subset in which *no two distinct elements are comparable*: for any
`x ≠ y` in the set, you have neither `x ≤ y` nor `y ≤ x`. In progress-tracking terms it is the
set of "tied / incomparable" boundary points — the states that are all genuinely on the
frontier because none of them is behind any of the others. For totally-ordered values like
`u64` an antichain can only ever hold one element (any two numbers are comparable), which is
why single-watermark frontiers are so cheap; the multi-element case only appears with truly
multi-dimensional progress.

### 25. What is a partial order, in everyday terms?

It is a way of saying "x is at least as far along as y" that is allowed to leave some pairs
**unranked**. With ordinary numbers, any two values can be compared — that's a *total* order.
But with something like `(partition, offset)` pairs, some pairs simply can't be ranked:
`(part 0, offset 9)` and `(part 1, offset 2)` are each ahead on one axis and behind on the
other, so neither is "≤" the other. That's a *partial* order, and the word "partial" is just
acknowledging that incomparable ties are a normal, expected outcome rather than an error.

### 26. What is a lattice?

A lattice is a partially ordered set with a comforting guarantee: *every* pair of elements has
both a **greatest lower bound** (their `meet`) and a **least upper bound** (their `join`).
Intuitively, no matter which two states you pick, there is always a well-defined "most
conservative common ground they're both past" and a well-defined "most optimistic combination
that covers both." That guarantee is what lets you combine *any* two progress reports and
always get a sensible, unique answer — never an undefined or ambiguous merge.

### 27. What is a "greatest lower bound"?

It is the largest value that is still ≤ *both* inputs. Take the numbers `5` and `8`: their
common lower bounds are `5, 4, 3, …`, and the *greatest* of those is `5`, so `meet(5, 8) = 5`,
which is just `min`. The phrase "greatest lower bound" is the version of that idea that keeps
working when plain `min` falls apart — for incomparable states like `(0,9)` and `(1,2)`, their
greatest lower bound is `(0,2)`, the richest point still behind both, even though neither
input is the "minimum."

### 28. What is a semilattice, and why do I keep seeing "semi"?

A **semilattice** is a set equipped with just *one* of the two operations — say, only `meet` —
that is commutative, associative, and idempotent. The reason it keeps coming up is that the
convergence guarantee only actually *needs* one direction: to agree on safe shared progress,
nodes only have to repeatedly `meet`. So the merge side of `antichain` is, strictly speaking,
a **meet-semilattice**. The full `Lattice` trait still provides both `meet` and `join` because
having both is convenient in practice — but the core correctness story rests on the simpler
semilattice half.

### 29. Why do commutativity, associativity, and idempotence delete the coordinator?

Because together they make the final result **completely independent of the schedule**. A
coordinator exists for exactly one reason: to impose an order on events ("collect everyone's
report, *then* compute the answer"). But if reordering the inputs can't change the result
(commutativity), regrouping them can't change it (associativity), and repeating them can't
change it (idempotence), then there is simply nothing left for a coordinator to decide. Any
node can mash together whatever updates it happens to have heard, in whatever order they
arrived, and land on the same answer as every other node. The order-imposing referee becomes
redundant.

### 30. What is "absorption" / domination?

Domination is the relationship "`a` is at least as far along as `b`" (formally `b ≤ a`). When
that holds, combining them by `join` just gives `a` straight back — `a` *absorbs* `b`, because
`b` adds no new information. This is the mechanism that keeps antichains minimal: when you
insert a new element that dominates an existing one, the old one is **replaced**; when you
insert one that is itself dominated, it is simply **dropped**. The set only ever retains the
truly incomparable "Pareto frontier" points, with all the redundant ones absorbed away.

### 31. What is the "consistency law" the docs mention?

It is a single biconditional that ties the order and the two operations together into one
truth:

$$a \le b \iff \mathrm{meet}(a, b) = a \iff \mathrm{join}(a, b) = b.$$

In words: "`a` is below `b`" is the *same statement* as "meeting them gives back `a`" and as
"joining them gives back `b`." Every public type in the crate is property-tested to satisfy
this in **both** directions. It earns that special attention because it is the law most likely
to quietly break in a hand-written lattice — if someone's custom `meet` and their `≤` disagree,
this is the test that catches it.

### 32. Why is `meet` the "safe" choice and `join` the "optimistic" one?

Picture the lattice as a vertical landscape with "no progress" at the bottom and "fully done"
at the top. `meet` always moves *downward*, toward the most conservative point that everyone
has definitely passed — so acting on anything below the `meet` is guaranteed safe, because no
worker is behind it. `join` moves *upward*, toward the furthest point anyone has reached —
which is great for advancing your own knowledge, but it is emphatically *not* a promise that
everyone is there yet. That directional difference is why pipelines publish behind the `meet`
and track their best-known progress with the `join`.

### 33. What is the "bottom" element?

`⊥` ("bottom") is the least element of the lattice — *"no progress yet; nothing is complete."*
For a `Frontier`, `Frontier::bottom()` is the empty frontier, the honest starting state before
any worker has reported anything. It is the natural seed value you begin folding from as
updates arrive, and it behaves as the identity for the `join`-based accumulation that advances
your knowledge upward from "nothing known" toward "everything seen."

### 34. What is the "top" element?

`⊤` ("top") is the greatest element — *"completely done / sealed / closed."* Not every type
comes with one: there is no natural "largest" `u64` that genuinely means "this stream is
finished forever" (using `u64::MAX` for that is a hack waiting to bite you). That's exactly
why `WithTop<T>` exists — it bolts a real, structural `Top` onto any type so you can say "this
partition is permanently sealed" cleanly and unambiguously, without overloading a magic
sentinel value that some other piece of code might mistake for an ordinary timestamp.

### 35. What's the minimal contract a type `T` must satisfy to be used?

To put a type inside an `Antichain` or `Frontier`, `T` only needs `PartialOrd + Clone` — the
library has to be able to ask "is this ≤ that?" and to copy values. To go further and get
value-level `meet`/`join`, `T` must implement the crate's `Lattice` trait, which spells out
how to compute the greatest lower and least upper bounds. The property tests document the
exact algebraic laws a correct `Lattice` implementation must obey, so if you write your own,
you can lift those same tests to prove yours behaves.

### 36. Does the partial order have to be a total order?

No — supporting *non*-total orders is the entire reason this library exists. Totally-ordered
types like `u64` are simply the easy special case: because any two values are comparable, the
antichain can never hold more than one element, and `meet` degenerates to `min`. Partially
ordered types are where frontiers actually earn their keep, holding several incomparable
boundary points at once to describe progress that is genuinely ahead in some dimensions and
behind in others. If everything you track were totally ordered, you wouldn't need antichains
at all — a single number would do.

### 37. Is there a formal definition of the convergence theorem?

Yes, and it is stated precisely: *"If two nodes have each observed any subset of the same
update set, in any order, then after merging all the updates they have, their `Frontier`
values are identical."* This is the formal backbone of the "you can delete the coordinator"
claim. It is written out in the README and — importantly — not merely asserted but mechanically
checked: the Fizzbee specification at
[`specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz) enumerates the
interleavings and confirms the property holds in every reachable state.

---

## For software engineers: using the library

### 38. How do I add it to my project?

Add it like any other crate:

```toml
[dependencies]
antichain = "0.3"
```

If you need to serialize frontiers (to send them over the wire or persist them), turn on the
optional feature: `antichain = { version = "0.3", features = ["serde"] }`. If you're targeting
an embedded or kernel context without the standard library, disable defaults to get a
`no_std` build: `antichain = { version = "0.3", default-features = false }`. The core type
pulls in essentially no runtime dependencies, so it won't bloat your dependency tree.

### 39. What's the smallest useful example?

This snippet shows the two things you'll do most — combining two progress claims with `meet`,
and checking that the result behaves as expected:

```rust
use antichain::Frontier;

let a = Frontier::from_elem(120u64);
let b = Frontier::from_elem(95u64);
let global = a.meet(&b);               // most conservative shared progress

assert!(global.less_equal(&95));       // 95 may still be in-flight
assert_eq!(a.meet(&b), b.meet(&a));    // order never matters
```

The takeaway: `meet` keeps the conservative `95`, `less_equal` lets you ask "has progress
reached this point?", and swapping the operands changes nothing — the property that lets you
merge reports in whatever order they happen to arrive.

### 40. How do I build a frontier?

There are three constructors, covering the situations you'll actually hit:

- `Frontier::bottom()` — an empty frontier meaning "no progress yet," the right seed before
  any reports arrive.
- `Frontier::from_elem(t)` — a single progress point, the common case for one watermark or
  offset.
- `Frontier::from_elements(iter)` — builds from many points at once and automatically keeps
  only the minimal, mutually-incomparable ones, discarding anything dominated.

That last one is handy when you already have a pile of candidate boundary points and want the
library to prune them down to the honest frontier for you.

### 41. How do I merge many frontiers at once?

Fold `meet` across them. Because `meet` is associative and commutative, the order and grouping
are irrelevant — you always get the same global answer:

```rust
use antichain::Frontier;
let workers = [120u64, 95, 200, 88].map(Frontier::from_elem);
let global = workers.iter().cloned().reduce(|a, b| a.meet(&b)).unwrap();
assert_eq!(global, Frontier::from_elem(88u64));
```

The slowest worker (`88`) sets the safe global progress, exactly as you'd want: you can act on
everything up to `88` knowing no worker is behind that line. This `reduce`-over-`meet` pattern
is the workhorse for collapsing a fleet of reports into one number.

### 42. What does `less_equal` tell me?

`frontier.less_equal(&t)` answers the question *"has the frontier reached or covered `t`?"* —
that is, is `t` at or below the frontier's boundary. Concretely it returns `true` when `t` is
`≤` some element of the antichain. This is your everyday "is it safe to act on `t` yet?" check:
if `less_equal(&deadline)` is true, every worker has passed `deadline` and you can fire
whatever depends on it (close the window, publish the report, garbage-collect the state).

### 43. How do I read what's inside a frontier?

Call `frontier.elements()`, which returns the slice of antichain points. For a totally-ordered
`Frontier<u64>` that slice holds exactly one value — the current watermark. For a
multi-dimensional frontier it holds the full set of incomparable boundary points, letting you
inspect each "tied" position. This is the read side you'll use for logging, metrics, or
driving downstream decisions off the actual frontier contents rather than just `less_equal`
checks.

### 44. When do I use `Antichain<T>` directly instead of `Frontier<T>`?

Reach for `Frontier` for almost everything — it's the ergonomic, progress-flavoured wrapper.
Drop down to `Antichain<T>` when you want the raw invariant-maintaining set itself, typically
because you're building a custom structure on top of it. At that level you work with
`empty()`, `from_elem()`, `insert()`, `elements()`, `len()`, `is_empty()`, and `less_equal()`,
and you get the automatic minimality guarantee without the `Frontier` semantics layered on
top. In short: `Frontier` for application code, `Antichain` when you're building new building
blocks.

### 45. What does `Antichain::insert` do with the invariant?

It keeps the set minimal for you, automatically, on every insert. If you insert an element
that is already dominated by something in the set, the insert is a no-op — it adds no
information. If you insert an element that dominates one or more existing members, those
dominated members are removed. The upshot is that you never have to deduplicate, sort, or
prune by hand: the antichain is *always* in its minimal canonical form, holding only the
truly incomparable points, no matter what sequence of inserts you throw at it.

### 46. How do I represent two independent dimensions?

Use `ProductTimestamp<A, B>`, which implements the **product order**, rather than a plain
tuple. This matters because standard tuples compare *lexicographically* — a completely
different order that would give wrong answers for genuinely independent axes:

```rust
use antichain::{Frontier, ProductTimestamp};
type Pt = ProductTimestamp<u64, u64>;          // (partition, offset)
let merged = Frontier::from_elem(Pt::new(1, 50)).meet(&Frontier::from_elem(Pt::new(2, 30)));
assert_eq!(merged.elements().len(), 2);        // incomparable → both kept
```

Because `(1, 50)` and `(2, 30)` are each ahead on one axis and behind on the other, neither
dominates, so the honest frontier keeps both — precisely what you want when the two dimensions
(say, partition and offset) really are independent.

### 47. What if the outer dimension should dominate the inner one?

Then you want `Lexicographic<A, B>`, where advancing the outer value makes the inner one
irrelevant. This is the right model for things like `(epoch, offset)`: once the epoch ticks
forward, the old offset no longer matters at all. Because the outer dimension always decides
comparisons, the antichain stays at width 1:

```rust
use antichain::{Frontier, Lexicographic};
type Clock = Lexicographic<u64, u64>;          // (epoch, offset)
let merged = Frontier::from_elem(Clock::new(4, 900)).meet(&Frontier::from_elem(Clock::new(5, 10)));
assert_eq!(merged.elements(), &[Clock::new(4, 900)]); // epoch 4 < epoch 5 outright
```

Epoch `4` is unambiguously behind epoch `5`, so the conservative `meet` keeps `(4, 900)` whole
— the large offset `900` doesn't rescue it, because the outer epoch dominates.

### 48. My set of workers/shards changes at runtime. What do I use?

Reach for `MapLattice<K, V>` — a per-key lattice backed by a `BTreeMap`. Each shard or worker
shows up the moment it first reports and disappears from the conservative view when it's no
longer universally present. The two operations have intuitive meanings here: `meet` is key
**intersection** plus a value-meet ("what *everyone* agrees on"), while `join` is key
**union** plus a value-join ("the furthest *anyone* has reached"). This is the go-to type
whenever your topology isn't fixed — autoscaling workers, churning shards, dynamic partitions.

### 49. How do I track which discrete members have acknowledged?

Use `SetLattice<T>`, whose partial order is subset inclusion. `meet` computes the
**intersection** ("acknowledged by *everyone*") and `join` computes the **union**
("acknowledged *somewhere*"). That makes it a natural fit for quorum and acknowledgement
tracking: collect each node's set of received acks, `meet` them to find the IDs that every
node has confirmed, and you have a coordinator-free "committed everywhere" set.

### 50. I want "everyone is *at least* here," not "at most here." How?

Wrap the value in `Max<T>`, which **inverts** the order so that `meet` keeps the **higher**
value — giving you a guaranteed floor ("every worker has reached at least this point") instead
of a conservative ceiling. This is handy when your invariant is a lower bound: a minimum
replication level reached everywhere, or a smallest acknowledged version. If you need to carry
*both* a lower and an upper bound through a single frontier, pair `Max<T>` with `Min<T>` in a
tuple, e.g. `(Max<u64>, Min<u64>)`, and each component tracks its own direction.

### 51. How do I signal "this stream is permanently closed" or "not started"?

Use the sentinel wrappers. `WithTop<T>` adds a `Top` value meaning closed/sealed/EOF: `Top`
absorbs `join` (once closed, always closed) and is the identity for `meet`. `WithBottom<T>`
adds a `Bottom` value meaning not-started: `Bottom` absorbs `meet` and is the identity for
`join`. When you need both ends — a value that can be "not started," then a real timestamp,
then "sealed forever" — compose them as `WithTop<WithBottom<T>>` to get a clean
`Bottom < Value(t) < Top` lattice. The big win is that you express these states *structurally*
rather than smuggling them in as magic constants like `0` or `u64::MAX` that other code might
misinterpret.

### 52. How do I track out-of-order ranges with gaps (e.g. backfill)?

Use `IntervalSetLattice<T>` from the companion crate
[`antichain-intervals`](../crates/antichain-intervals). Instead of a single boundary, it
maintains a canonical set of disjoint intervals, so it can faithfully represent "I've got
100–150 and 200–250, but nothing in between." `join` coalesces overlapping or adjacent ranges
as new data lands, and `meet` intersects them. It's the right tool for backfill and
out-of-order ingestion — for instance when block 150 arrives before block 101 and you need to
track exactly which gaps remain rather than pretending progress is a single contiguous line.

### 53. Can I clamp a value to a finite range?

Yes — `Bounded<T>` clamps every value to a fixed `[min, max]` range at construction time.
Inputs outside the range are clamped to the nearest endpoint rather than rejected, so you
never have to handle an error path. A nice consequence of the range being finite is that the
antichain width is *provably* bounded by the range's cardinality — it can't blow up. The one
rule to follow is that all values living in the same antichain must share the **same** range,
since the bounds are part of how comparisons are defined.

### 54. Do I have to handle de-duplication of messages myself?

No — and this is one of the most freeing properties in practice. Because `meet` is idempotent,
re-merging a value you've already incorporated has exactly zero effect on your result. That
means you can run on cheap at-least-once delivery — retries, gossip, overlapping broadcasts —
without maintaining any "have I seen this already?" bookkeeping. Duplicates aren't a hazard you
have to defend against; they're simply absorbed harmlessly by the merge.

### 55. Is `Frontier` cheap to clone?

For totally-ordered types like `Frontier<u64>` it is allocation-free — the data lives inline,
so cloning is trivially cheap. For genuinely partially-ordered frontiers of width ≥ 2 it clones
a small `Vec`, which is still inexpensive because widths stay small in practice (typically in
the single or low double digits, ≤ ~50). So in the overwhelmingly common cases, cloning a
frontier to ship or store it costs almost nothing. (See [§61](#61-how-fast-is-meet) for the
measured numbers.)

### 56. Is the API stable? Will my code break?

The crate follows semantic versioning and runs `cargo-semver-checks` in CI to catch accidental
breakage, so within `0.3.x` the public API is stable and patch upgrades are safe. Genuinely
breaking changes bump the minor version (the `0.x` semver convention) and are always recorded
in the [CHANGELOG](../CHANGELOG.md). The practical advice for any `0.x` dependency still
applies: pin a version you've tested, and read the changelog before bumping the minor.

---

## Choosing and composing types

### 57. There are a lot of types. How do I pick one?

The fastest path is the decision table in the
**[Cookbook](cookbook.md#decision-table--which-type-do-i-use)**, but here's the short version
you can scan in a few seconds. Match the *shape* of your progress to the row, and the type
falls out:

| You have… | Reach for |
|-----------|-----------|
| One watermark / offset / clock | `Frontier<u64>` |
| Two independent dimensions | `ProductTimestamp<A, B>` |
| Outer dominates, inner breaks ties | `Lexicographic<A, B>` |
| A topology that rescales at runtime | `MapLattice<K, V>` |
| Which members acknowledged | `SetLattice<T>` |
| A lower bound (merge by `max`) | `Max<T>` (and `Min<T>`) |
| A value in a finite range | `Bounded<T>` |
| A stream that can close / hasn't started | `WithTop<T>` / `WithBottom<T>` |
| Out-of-order progress with gaps | `IntervalSetLattice<T>` |

If more than one row seems to fit, it's usually because your progress is genuinely composite —
which is fine, because these types are designed to nest (see the next question).

### 58. Can I combine these types?

Yes — composition is the entire point, and it's where the design really pays off. You can nest
these types arbitrarily, and the composite's partial order — along with the all-important
convergence guarantee — is derived *automatically* from its parts, with no extra work or proof
obligation on your side. A few realistic examples: `Frontier<(Max<u64>, Min<u64>)>` carries a
floor and a ceiling together; `MapLattice<ShardId, ProductTimestamp<u64, u64>>` tracks a
two-dimensional progress point per dynamically-appearing shard; `WithTop<WithBottom<u64>>`
gives you a watermark that can also be "not started" or "sealed." Build the type that mirrors
your problem, and the correctness comes along for free.

### 59. Why shouldn't I just use a plain tuple `(A, B)`?

Because a standard-library tuple compares **lexicographically** — it ranks by the first
component and only consults the second to break ties — whereas independent dimensions need the
**product order**, where neither component dominates. The difference isn't cosmetic: a
component-wise `meet` on a lexicographic tuple is *not* a true greatest lower bound, so it
quietly violates the algebra the whole library depends on. Use `ProductTimestamp<A, B>` when
your axes are independent (partition and offset), and reach for `Lexicographic<A, B>` only
when you genuinely *want* the outer component to dominate (epoch then offset). Picking the
wrong one produces subtly wrong merges that are painful to debug later.

### 60. When should I use `MapLattice` instead of widening a `Frontier`?

Use `MapLattice` when the number of dimensions is **dynamic** — shards spin up and down,
partitions come and go — or when widening a single antichain would make it grow large because
each "dimension" is really a separate keyed channel rather than a coordinate in a shared
space. Keying by a `MapLattice` keeps each per-key value cleanly totally-ordered and the
overall structure easy to reason about, instead of cramming everything into one wide antichain
whose elements are hard to interpret. Rule of thumb: independent *coordinates* of one point →
`ProductTimestamp`; independent *channels* that appear and vanish → `MapLattice`.

### 61. What's the difference between `Max<T>` and `Min<T>` again?

`Max<T>` actively **inverts** the order so that `meet` computes the `max` — giving you a
guaranteed lower bound or floor ("everyone has reached at least here"). `Min<T>`, by contrast,
is a *transparent* newtype that preserves the natural order; its value is essentially
documentary, signalling intent and pairing cleanly with `Max<T>` inside composites like
`(Max<u64>, Min<u64>)`. So the asymmetry is deliberate: `Max` does real work flipping the
direction of "safe," while `Min` mostly exists to make a two-sided bound read clearly.

### 62. Why does `Bounded<T>` need all values to share the same range?

Because `Bounded<T>`'s lattice operations are computed relative to `self`'s own `[min, max]`
bounds. If you mixed values carrying *different* ranges in one antichain, the comparisons
would reference inconsistent endpoints and the result would be undefined by design. The fix is
simple and cheap: decide the range once, up front, and construct every value in that antichain
with the same bounds. As long as the range is consistent, you also get the bonus guarantee
that the antichain width can't exceed the range's cardinality.

---

## Performance and internals

### 63. How fast is `meet`?

For the common totally-ordered case (`Frontier<u64>`) it is effectively **O(1)** and entirely
allocation-free — the antichain collapses to a single element, so a merge is little more than a
comparison. For partially-ordered types the cost is **O(n·m)** in the two antichain widths,
since each element of one side may need checking against each element of the other. In
absolute terms that's still fast at realistic widths. Measured on an Apple M-series chip in a
release build:

| Operation | Width 10 | Width 100 | Width 1000 |
|-----------|----------|-----------|------------|
| `Frontier<ProductTimestamp>::meet` | 147 ns | 9.2 µs | 825 µs |
| `Frontier<u64>::meet` (width 1) | 18 ns | 18 ns | 18 ns |

Notice the single-value case is flat at ~18 ns regardless of "width" — because it never widens —
while the multi-dimensional case only becomes noticeable at widths far beyond what real
systems produce.

### 64. Will the antichain "explode" in size for multi-dimensional progress?

In practice, no — and this was checked with data rather than left to hope. Empirically, widths
stay at or below roughly 50 for real workloads, a regime where `meet` costs well under a
microsecond. The reason it doesn't blow up is that genuinely incomparable progress points are
rare in well-modelled systems; most reports either dominate or are dominated, so the antichain
keeps pruning itself back down. If you do have a dimension that can grow without bound, the
right move is to model it as a `MapLattice` key rather than letting the antichain widen. This
was flagged as the single highest-risk question in the roadmap and was closed with benchmark
evidence.

### 65. Does it allocate memory?

Only when it genuinely has to. Width-0 and width-1 antichains are stored **inline with zero
heap allocation**, so the overwhelmingly common single-watermark case never touches the
allocator at all. Only genuinely partially-ordered antichains of width ≥ 2 spill over into a
`Vec`. And the behaviour is symmetric: when a wide antichain later shrinks back down (because
new reports dominate the old incomparable points), it *renormalizes* to the allocation-free
inline representation again. You pay for heap memory exactly when you're using the
multi-dimensional capability and not a moment longer.

### 66. Is there a compaction step on `meet`?

No — and the benchmarks confirmed one isn't needed. At width ≤ 100, a `meet` costs under 10 µs,
and beyond that you've already exceeded the widths real systems produce, so a runtime
compaction pass would be optimizing a case that doesn't occur. If you *do* find a dimension
driving genuine width growth, the recommended remedy is **structural** — move that dimension
into a `MapLattice` key so each value stays totally-ordered — rather than bolting on a
compaction phase that would add complexity and cost to the common path for no real benefit.

### 67. Is it `no_std` compatible?

Yes. Disable the default `std` feature and the crate runs in `no_std` environments, requiring
only `alloc` (a global allocator) for the cases that need heap storage. Both the core crate
and the companion `antichain-intervals` support `no_std`, and CI actively builds that
configuration so it can't silently regress. That makes the primitive usable in embedded
firmware, OS kernels, WASM, and other constrained contexts where the full standard library
isn't available.

### 68. Does it use any `unsafe` code?

No — and this is enforced, not merely intended. Both crates carry
`#![forbid(unsafe_code)]` at the crate root, which makes the compiler *reject* any `unsafe`
block anywhere in the source. So there is provably zero unsafe code to audit, no chance of a
memory-safety bug hiding in a hand-tuned hot path, and no caveats to worry about when vetting
the crate for a safety-critical or security-sensitive project.

### 69. What are the runtime dependencies?

Effectively none for the core data type — that minimalism is deliberate. `serde` is the only
optional dependency and it is feature-gated, so it's absent unless you ask for it. Everything
else (`proptest`, `criterion`, `serde_json`) is dev-only, used purely for tests and
benchmarks and never compiled into your binary. The result is a boring, portable primitive
that won't drag a transitive dependency tree into your build or expose you to churn from
upstream crates.

### 70. How big is the codebase?

Small and auditable by design — a single-file core plus the companion intervals crate, the kind
of surface one person can read in a sitting. The guiding philosophy is "the math is where the
certainty lives": rather than growing a large, feature-rich API that's hard to verify, the
project keeps the primitive tiny and proves it correct. A small, proven core that you compose
upward beats a sprawling one you have to trust.

---

## Correctness, testing, and formal proofs

### 71. How do I know the algebra is actually correct?

Every public type is **property-tested** — the test harness throws 10,000+ randomly generated
cases at each one and checks the laws hold every single time. The properties under test are
the full algebraic contract: commutativity, associativity, idempotence, absorption, the
antichain minimality invariant, and the universal consistency law
`a ≤ b ⟺ meet(a,b)==a ⟺ join(a,b)==b` — verified in *both* directions. Rather than trusting a
handful of hand-picked examples, this hammers the real implementation with adversarial random
input, which is how subtle ordering or domination bugs get surfaced.

### 72. Is the convergence guarantee actually proven?

Yes — mechanically, not just by argument. A Fizzbee model-checking specification
([`specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz)) exhaustively
enumerates *every* possible interleaving of update deliveries across nodes and asserts that
convergence holds in every reachable state. Because it explores the entire state space rather
than sampling it, there is no adversarial message ordering — however perverse — that can sneak
through and cause two nodes to diverge. The claim "you can delete the coordinator" isn't a
hopeful assertion; it's a checked property.

### 73. How do I run the formal check myself?

Install Fizzbee and run the spec — two commands:

```sh
brew tap fizzbee-io/fizzbee && brew install fizzbee
fizz specs/frontier_convergence.fizz
```

The checker will explore the model's reachable states and confirm the convergence assertion
holds throughout. This is something you can run yourself to gain independent confidence rather
than taking the project's word for it — the proof is in the repository, not hidden behind a
badge.

### 74. What's the difference between the property tests and the formal spec?

The two cover complementary risks. The **property tests** throw thousands of *random* inputs
at the actual Rust code, verifying that the real implementation — the bytes that ship — obeys
the algebraic laws. The **formal spec** instead takes an abstract model and *exhaustively*
explores all possible message orderings, verifying that the *design* is sound under every
conceivable schedule. Property tests answer "does the implementation behave?"; the spec
answers "is the design correct in principle?" Having both means a bug would have to slip past
both random execution and exhaustive model-checking, which is a high bar.

### 75. Is the library fuzzed?

Yes — there are `cargo-fuzz` targets (in `fuzz/`) that exercise the `insert` and `meet` code
paths with adversarial, coverage-guided input. Their job is to hunt for panics, crashes, or
violations of the antichain invariant that structured tests might not think to construct.
Fuzzing complements the property tests and the formal spec by attacking the implementation
from yet another angle — "can any byte sequence at all break this?"

### 76. Are the documentation examples tested?

Yes — the code blocks in the Cookbook and Tutorial are compiled and executed as doctests (wired
in via a `#[cfg(doctest)]` include), so they're checked on every test run. This means the
examples in the docs can't silently rot out of sync with the code: if an API changes in a way
that breaks a documented snippet, the build fails. When you copy an example from the docs, you
can trust it actually compiles and runs against the version you're reading about.

### 77. What does CI enforce?

The CI pipeline is strict and covers the whole quality surface: `cargo fmt --check` for
formatting, `cargo clippy --workspace --all-targets --all-features -D warnings` (warnings are
hard errors, and test code must be lint-clean too), the full test suite — *including* a
`--no-default-features` run to keep the `no_std` build honest — a dedicated MSRV `cargo check`
so the minimum Rust version can't drift, and `cargo-semver-checks` to catch accidental API
breakage. In short, formatting, lints, tests, `no_std`, MSRV, and semver are all gated, so
regressions in any of them block the merge.

---

## How it compares to other tools

### 78. How does this differ from timely-dataflow / differential-dataflow?

Those are the direct intellectual ancestors — they pioneered this exact `Antichain`/`Frontier`
algebra — but they bake it into a **full dataflow runtime** complete with scheduling,
communication, and worker management. `antichain` extracts *just the progress primitive* and
nothing else: zero runtime dependencies, `no_std` support, a broader toolkit of composable
lattice types, and a formally model-checked convergence spec. The choice is about scope. If
you want a complete stream-processing engine and are happy to adopt its whole model, use
timely/differential. If you just want the proven progress-tracking building block to drop into
*your own* system — a Kafka consumer, a custom replicator, an embedded device — use
`antichain`.

### 79. How is this different from a CRDT library?

Same algebra, different target. CRDT libraries replicate application **data** — counters,
sets, maps — and usually merge by `join` so replicas grow toward agreement. `antichain` tracks
**progress** — "how far has the computation gotten" — and usually merges by `meet` to find the
safe shared frontier. They're complementary rather than competing, and a very common pattern
is to use both together: a CRDT to hold the replicated data, and a `Frontier` to act as the
fence that tells you *when* that data is complete enough to read. The CRDT says what the value
is; the frontier says when you're allowed to trust it.

### 80. Isn't an `Antichain` just a priority queue or a sorted set?

No — it's a fundamentally different structure. A priority queue maintains a *total* order and
hands you a single min or max. A sorted set orders *every* element relative to every other.
An antichain instead keeps the **Pareto frontier** of a *partial* order — precisely the set of
mutually-incomparable elements, the ones where none dominates another. The defining feature is
its invariant-maintaining `insert`, which automatically drops dominated points and evicts
elements that a newcomer dominates. Neither a priority queue nor a sorted set does that, which
is exactly why a purpose-built type exists.

### 81. When should I *not* use this crate?

When your problem is really about **ownership, membership, or consensus** — questions like
"*who* is allowed to write to shard 42?", "what happens when a node crashes mid-write?", or
"how do we elect a leader?" Those need leases, quorums, or a consensus protocol like Raft, and
`antichain` deliberately doesn't pretend to solve them. It tracks *progress*, not *authority*.
If you find yourself reaching for it to decide who's in charge or to guarantee mutual
exclusion, that's a sign you've crossed out of its problem domain and need a different tool.

### 82. Can I use it alongside Raft / a consensus system?

Absolutely — they're complementary, and pairing them is a clean architecture. Use a consensus
system for the **control plane**: membership, leader election, who owns which shard, the
decisions that genuinely require agreement on a single authoritative answer. Then use
`antichain` for the **data-plane** progress tracking that *doesn't* need a coordinator — the
high-frequency "how far has everyone gotten" question that would otherwise bottleneck on the
consensus layer. Keeping that seam sharp — consensus for authority, lattice merge for progress
— is one of the project's core design principles.

### 83. Where can I read a fuller comparison?

See [`docs/comparison.md`](comparison.md) for a fair, strawman-free comparison against
timely/differential-dataflow and CRDT libraries. It lays out honestly what each approach is
better at — not a sales pitch — so you can decide based on your actual constraints rather than
on marketing. If you're weighing `antichain` against an existing engine or library, that's the
document to read before committing.

---

## Project, packaging, and practical matters

### 84. What version is current?

At the time of writing, `antichain` is at `0.3.0` and the companion `antichain-intervals` is
at `0.1.0`. The two are versioned independently because they evolve at different rates — the
core moves more deliberately than the newer intervals crate. For the full version history all
the way back to `0.1.0`, including what changed in each release and any migration notes,
consult the [CHANGELOG](../CHANGELOG.md) before upgrading.

### 85. What is the Minimum Supported Rust Version (MSRV)?

The MSRV is Rust `1.85` (edition 2024). This isn't just a number in the README — CI runs a
dedicated MSRV `cargo check` job on every change, so the policy genuinely can't regress
without someone noticing and deciding to bump it on purpose. That means if you're pinned to
`1.85`, you can rely on the crate continuing to build for you, and any future MSRV increase
will be a visible, deliberate decision rather than an accidental drift.

### 86. What license is it under?

Apache-2.0 — a permissive, business-friendly license that includes an explicit patent grant.
It's a safe choice for both open-source and commercial projects, so you can depend on the
crate without licensing surprises.

### 87. How do I enable serialization?

Turn on the `serde` feature: `antichain = { version = "0.3", features = ["serde"] }`. Once
enabled, `Antichain` and `Frontier` serialize to a stable, self-describing wire format —
`{ "elements": [...] }` — so you can ship frontiers over the network or persist them to disk.
That format is locked down by round-trip tests, which means it won't quietly change shape
underneath you between releases and break data you've already written. You can safely store
serialized frontiers and read them back with a later version.

### 88. Does serde work in `no_std`?

Yes. The `serde` feature pulls in serde's `alloc`-based collection support, so it composes
cleanly with a `no_std` + `alloc` build — you get serialization even without the standard
library. (An earlier release had a latent bug in exactly this combination; it has since been
fixed and is now guarded by tests, so the `no_std` + serde path is covered rather than
aspirational.) If you're on embedded or WASM and need to serialize frontiers, this
combination is supported.

### 89. Why is there a separate `antichain-intervals` crate?

Because `IntervalSetLattice<T>` needs a non-trivial interval-coalescing data structure to
merge and split ranges, and folding that into the core would bloat what is otherwise a tiny,
dependency-light primitive. Splitting it into a companion crate keeps the core lean, minimal,
and `no_std`-simple, while the intervals crate still implements `antichain::Lattice` — so it
drops straight into a `Frontier` or `MapLattice` exactly like the built-in types. You only pay
for the interval machinery (in code size and dependencies) if you actually use it.

### 90. Where are the runnable examples?

In the [`examples/`](../examples) directory, each one a self-contained program you can run and
read:

- `watermark_gossip.rs` — N simulated workers gossiping over a lossy channel and converging to
  a shared watermark; a live, runnable demonstration of the convergence theorem in action.
- `backfill_gaps.rs` — out-of-order block arrival handled with `antichain-intervals`, showing
  how gaps are tracked and filled.
- `progress_protocol.rs` — a complete three-layer Worker → Shard → Cluster protocol built
  entirely on the public API, demonstrating how the primitives compose into a real system.

Run any of them with, for example, `cargo run --example watermark_gossip`. They're the
fastest way to see the concepts turn into working code.

### 91. Where's the API reference?

On [docs.rs/antichain](https://docs.rs/antichain), which hosts the full rustdoc generated
straight from the source. Every public item carries documentation with runnable examples and,
where relevant, the specific algebraic law it upholds explained inline. So you're not just
getting type signatures — you get the *why* alongside the *what*, right next to each method,
which makes it a genuinely useful reference rather than a bare listing.

### 92. How do I learn it properly, in order?

There's a deliberate learning path, from intuition to reference:

1. This FAQ — read the first three sections for the core mental model.
2. The **[Tutorial](tutorial.md)** — a narrative that builds up from "one number" to a full
   multi-dimensional frontier, so the concepts land in order.
3. The **[Cookbook](cookbook.md)** — pick-a-type recipes for concrete problems once you know
   the basics.
4. The **[Design notes](idea.md)** — the deeper algebra and the philosophy behind the
   project's scope boundaries.
5. The **[API docs](https://docs.rs/antichain)** — the exhaustive reference for day-to-day
   lookups.

Following that sequence takes you from "what is this for" to "how do I use it on my problem"
to "why is it designed this way" without skipping the intuition-building steps.

### 93. Can I contribute? What's the review bar?

Yes, contributions are welcome — but the bar is intentionally high on correctness, because the
whole value of the crate is that you can trust it. A new lattice type must arrive with the
full property-test suite (including the consistency law verified in both directions) and must
fit the project's "progress only, no consensus" scope — features that drift into ownership or
agreement territory will be declined on principle, not merit. If you're curious how the crate
was built and what's deliberately deferred, the [roadmap](../roadmap.md) documents the
phase-by-phase construction and the reasoning behind it.

### 94. What's explicitly *out of scope* for this crate?

Four categories are deliberately excluded: networking and gossip protocols, consensus /
leader-election / leases, storage engines, and query planners. None of these are oversights —
they are all perfectly legitimate things to *build on top of* the primitive, but they are not
the primitive itself. The project's central discipline is keeping that boundary sharp:
`antichain` provides the proven progress-merge building block, and you assemble the networking,
storage, and coordination around it. Resisting scope creep is what keeps the core small enough
to prove correct.

### 95. Is there a roadmap of what's next?

Yes — [`roadmap.md`](../roadmap.md). Phases 0 through 10 (core data type, formal proofs,
composition toolkit, hardening, the formal spec, extended lattices, performance work, adoption
docs, and onboarding) are all complete, which is why the crate is already a finished primitive
rather than a work in progress. Future work is intentionally demand-driven: additional
composition patterns will be added *only if* real downstream usage demonstrates a genuine
need, rather than speculatively growing the API. The bias is toward staying small and proven.

---

## Troubleshooting and common gotchas

### 96. My two-dimensional frontier has two elements and I expected one. Bug?

Almost certainly not — this is usually the library being *more* honest than you expected. If
the two points are genuinely incomparable under the product order (each is ahead on one axis
and behind on the other, so neither dominates), then keeping both is the *correct* behaviour:
together they describe the true boundary of completed work, and collapsing them to one would
throw away real information. For example, `(part 0, offset 9)` and `(part 1, offset 3)` simply
can't be merged into a single point without lying. If what you actually wanted was for the
outer dimension to dominate (so advancing it makes the inner one irrelevant), you've reached
for the wrong type — use `Lexicographic`, not `ProductTimestamp`.

### 97. I used a tuple and the merge result looks wrong.

That's the classic tuple trap. A plain `(A, B)` tuple compares **lexicographically** — it
ranks entirely by the first element and only consults the second to break ties — and a
component-wise `meet` over that order is *not* a true greatest lower bound. Because it
violates the algebra, the library deliberately excludes raw tuples from the universal
consistency law, which is the deeper reason your merges are coming out wrong. The fix is to
swap in `ProductTimestamp<A, B>` for independent dimensions (or `Lexicographic<A, B>` if you
actually wanted the outer to dominate). Once you use the right type, the merge behaves.

### 98. `meet` gave me a *smaller* number than both inputs with `Max<T>` — wait, no, a larger one.

That larger result is exactly right. `Max<T>` **inverts** the order, so `meet` on `Max` values
computes the **maximum**, not the minimum — that's the whole purpose of the wrapper: it flips
which direction "safe" points, turning your conservative merge into a guaranteed *floor*
("everyone has reached at least this high"). If you genuinely wanted the minimum, drop the
wrapper and use the bare value (or `Min<T>`, which keeps the natural order). Whenever a `Max`
merge surprises you, mentally flip the order and it'll make sense.

### 99. My `MapLattice` `meet` dropped keys I expected to keep.

That's `meet` doing its job: on a `MapLattice` it is a **key intersection**, so only keys
present in *both* maps survive (and their values are met together). The result is the
conservative "what does *everyone* have in common" answer — a key that only one side knew about
can't be part of what both agree on, so it's dropped. If instead you wanted to keep every key
from either side ("the furthest *anyone* has reached"), you want `join`, which is the key
**union**. Picking `meet` vs `join` here is really picking "intersection vs union," so choose
based on which question you're asking.

### 100. `--features serde` won't compile in my older setup.

First, make sure you're on `0.3.x`: an earlier release didn't wire up serde's `alloc`
implementations correctly for `MapLattice` and `SetLattice`, and upgrading is the fix. Second,
if you're building for `no_std`, remember that serde needs its `alloc` support to serialize
collections — the `serde` feature now enables that for you automatically, so you don't have to
hand-configure it. Between those two points, the vast majority of "serde won't compile"
reports resolve to "upgrade to current `0.3.x`."

### 101. I'm getting a width-explosion in a high-dimensional frontier.

The cause is almost always trying to widen a single antichain across a dimension that can grow
without bound — that's the one scenario where width gets out of hand. The remedy is
structural, not a runtime tweak. If the runaway dimension is an open-ended set of channels
(shards, partitions, keys), move it into a `MapLattice` key so each per-key value stays neatly
totally-ordered and the antichain never widens. If instead it's a *finite* range of values,
wrap it in `Bounded<T>`, which caps the antichain width at the range's cardinality by
construction. Either way, you're putting a structural lid on growth rather than hoping it
stays small.

### 102. Why does `Frontier::bottom()` say nothing is complete, but it's the starting point?

Those two facts are the same fact. `bottom()` is `⊥` — *"no progress yet"* — which is precisely
the honest state you should start from *before* any worker has reported anything. It's the
identity you fold updates into, and as reports arrive you build *upward* from it. The thing to
avoid is conflating "bottom of the lattice" (least progress, the seed) with "the answer": you
don't read `bottom()` as a result, you accumulate away from it. Starting at "nothing is done"
and climbing as evidence arrives is exactly the right model.

### 103. Is `less_equal` asking "is the frontier past t" or "is t past the frontier"?

It's asking whether the frontier has **reached or covered** `t`. Concretely,
`frontier.less_equal(&t)` returns `true` when `t ≤ some element` of the frontier — read it
out loud as *"has progress arrived at `t`?"* A `true` means it's safe to act on `t` because
the frontier has gotten there; a `false` means `t` is strictly beyond the frontier and hasn't
been reached yet. If you keep the phrasing "has progress *arrived at* `t`?" in mind, the
direction stops being confusing.

### 104. Where do I report a bug or ask a question not covered here?

Open an issue on the GitHub repository — that's the place for both bug reports and questions
the docs don't answer. If your report is about correctness, the single most useful thing you
can attach is a failing `proptest` seed or a minimal reproduction, because that lets the
behaviour be reproduced and fixed immediately rather than guessed at. Concrete repros turn a
vague "this seems wrong" into a test case that can be added to the suite.

---

*Didn't find your question? The [Tutorial](tutorial.md) builds the intuition from scratch, the
[Cookbook](cookbook.md) maps problems to types, and [`idea.md`](idea.md) explains the
philosophy and scope.*
