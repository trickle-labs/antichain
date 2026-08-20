# GLOSSARY

## Ubiquitous Language for `antichain`

This document is the shared vocabulary for the project. It describes the words we use when
we talk about distributed progress, the order structures that make it mergeable, and the
systems that can be built around the crate. The code uses precise mathematical meanings, but
those meanings are meant to make engineering conversations clearer, not to make ordinary
work feel like a proof seminar. When a term has a narrower meaning here than it has in general
systems literature, this document calls that out.

The central idea is simple: a system can record where several workers have got to, then merge
their progress claims without appointing one worker as the keeper of truth. The rest of the
vocabulary explains what a progress claim contains, what it means to combine two claims, and
how to choose an order that matches the shape of the work.

## The core vocabulary

### Antichain

An antichain is a set of values in which no value is comparable with another under the chosen
partial order. In practical terms, every value remains because none of the others can honestly
say that it is ahead of, behind, or otherwise a stronger version of it. A pair such as
`(partition 0, offset 100)` and `(partition 1, offset 20)` may be incomparable when the two
partitions advance independently. Keeping both points preserves information that a single
number would erase.

`Antichain<T>` is the crate's container for this idea. It maintains the invariant for you: an
inserted value that is already covered by an existing value is ignored, while a value that
covers existing values removes those redundant values. For a totally ordered timestamp such
as `u64`, the container usually has width one because every pair can be ranked. For a genuinely
multi-dimensional timestamp, its width is the number of boundary points that are still needed
to describe the shape of progress.

### Boundary

A boundary is the line separating work that a progress claim says is complete from work that
may still be in flight. Boundaries are represented by one or more timestamp values. The word
is deliberately neutral about the domain: a boundary can be a log offset, an event-time value,
a pair of partition and offset coordinates, or a set of covered intervals.

The boundary is not itself a report that every operation at that exact value has completed. In
the ordinary `Frontier<T>` interpretation, timestamps strictly below a boundary element are
complete, while a timestamp less than or equal to a boundary element may still be in flight.
That strictness matters when a consumer decides whether it can emit, acknowledge, compact, or
garbage-collect something at the edge.

### Comparable and incomparable

Two values are comparable when the selected partial order can place one below the other, or
when they are equal. They are incomparable when neither value is less than or equal to the
other. Incomparability is not an error and does not mean the values are malformed. It is the
honest result when independent dimensions have advanced in different directions.

The order comes from the domain model, not from the shape of a Rust value. A standard tuple has
lexicographic ordering, so it treats its first component as decisive. `ProductTimestamp` uses
product ordering instead: one value is below another only when both components are below or
equal. Choosing between those two orders is a semantic decision, not a cosmetic type choice.

### Coordinator-free

Coordinator-free means that the merge operation does not require one process to collect every
report, decide the answer, and distribute it again. Workers or replicas may exchange values
through gossip, a manifest, a queue, object storage, or an application-specific transport.
The transport can still have an owner and an operational topology; coordinator-free describes
the algebra of the merge, not the absence of all infrastructure.

The phrase also does not promise that every surrounding protocol is coordination-free. A
system may use `antichain` for progress and still need a lease, a membership service, or a
compare-and-swap operation to decide who may write. The crate supplies a mergeable value type,
not a complete distributed runtime.

### Convergence

Convergence is the property that makes coordinator-free merging useful. Two nodes that have
absorbed the same underlying set of progress facts reach the same value, even if they received
the facts in different orders, received duplicates, or grouped the merges differently. A node
that has seen fewer facts can hold a different, usually more conservative, value. Convergence
says that once the knowledge sets match, the answers match too.

For `antichain`, convergence follows from the laws of `meet`: commutativity makes delivery
order irrelevant, associativity makes grouping irrelevant, and idempotence makes duplicate
delivery harmless. The project treats this as an executable claim, supported by property tests
and the model-checked specification in `specs/frontier_convergence.fizz`.

### Dominates

We say that `x` dominates `y` when `x <= y` in the chosen partial order. A dominating value
is at least as conservative or informative for the antichain invariant, so `y` is redundant
when both are present. This wording is easy to reverse if it is read as ordinary progress
language: a numerically smaller timestamp can dominate a larger timestamp when the boundary
means "everything below here is complete."

The implementation uses this relationship during insertion. If an existing element is less
than or equal to the new value, the new value is dominated and is skipped. If the new value is
less than or equal to an existing element, the existing element is removed. Values that are
incomparable remain together.

### In flight

In flight means that the progress claim does not yet establish completion for the item. With
a `Frontier<T>`, `less_equal(time)` returns true when `time` is less than or equal to some
frontier element, which is the crate's operational test for a timestamp that may still be in
flight. The phrase does not say that work definitely exists or definitely will finish; it says
the frontier cannot certify that it is complete.

This distinction lets a consumer be conservative without pretending to know more than it does.
A reader can act on work strictly below the frontier, while leaving the boundary and everything
beyond it for a later observation. Applications may choose a different interpretation for a
custom lattice, but they should document the meaning of the order and boundary together.

### Lattice

A lattice is an ordered space in which any two values have both a greatest lower bound and a
least upper bound. In the crate, the `Lattice` trait names those operations `meet` and `join`.
The trait is useful because it lets the same merge reasoning apply to integers, sets, maps,
composite timestamps, and application-defined types.

A type does not become a good lattice merely because it has methods with these names. Its
operations must agree with its `PartialOrd` implementation and preserve the intended algebra.
For the systems use cases in this project, the important practical result is that a valid merge
can be repeated and reordered without changing the eventual answer.

### Meet

`meet` is the greatest lower bound of two values. It returns the most advanced value that is
still below or equal to both inputs in the selected order. When the values are ordinary
numbers, this is `min`. When the values are sets, it is intersection. When the values are
maps, it is the intersection of keys with a value-level meet for keys present on both sides.
The operation changes shape with the order, but its meaning stays the same.

`Frontier::meet` is the crate's central coordinator-free merge. For a `Frontier<u64>`, the
result is the more conservative boundary. For a frontier over an antichain, it retains the
minimal boundary needed to represent the common safe claim. Use it when the question is, "What
can I rely on across all of these reports?"

### Join

`join` is the least upper bound of two values. It returns the least value that is above or equal
to both inputs. For ordinary numbers it is `max`; for sets it is union; for maps it is key union
with a value-level join for overlapping keys. Join is often the right operation for accumulating
what any participant has observed or for advancing an optimistic local view.

In progress protocols, it is important not to confuse "furthest seen" with "safe for everyone."
A local node may use join to remember the broadest progress it has learned about, while using
meet to calculate the boundary it is allowed to publish or act on. The order and the desired
claim decide which operation is correct.

### Partial order

A partial order defines what it means for one progress state to be no greater than another. It
must be reflexive, antisymmetric, and transitive, but it does not require every pair to be
comparable. That last property is what allows the project to represent independent partitions
without inventing a false total ranking.

The partial order is the most important modeling choice in an `antichain` design. If the order
is wrong, the merge can be perfectly implemented and still answer the wrong question. Start
with the statement you want to make about progress, write down when one state safely implies
another, and then choose or compose the type whose order expresses that implication.

### Progress claim

A progress claim is a value that states how much work a participant can certify as complete.
`Frontier<T>` is the project's main progress-claim type. It wraps an antichain of timestamps
and gives those timestamps boundary semantics: everything strictly below at least one boundary
point is considered complete.

A progress claim is not a log of every completed item, and it is not a promise that the worker
will never discover more work behind the boundary. It is a compact statement that can be
stored, transmitted, merged, and compared. The surrounding application is responsible for
making sure the claim is backed by the durability and ownership rules its correctness requires.

### Timestamp

A timestamp is the value used to position progress in an ordered domain. It does not have to be
a wall-clock time. A timestamp may be a sequence number, an offset, an epoch-and-offset pair,
a partition coordinate, or a custom value that implements the required ordering operations.

The useful question is not "what time is it?" but "what does it mean for one timestamp to be
behind another?" `ProductTimestamp` and `Lexicographic` exist because two systems may use the
same pair of numbers while needing different answers to that question. A timestamp should
carry the causal or positional meaning needed by the progress protocol, rather than merely
being convenient data that happens to be sortable.

### Watermark

A watermark is a domain-specific name for a progress boundary, especially in stream processing.
It usually means that events below the watermark are complete enough for a downstream operator
to close a window or emit a result. In this project, a watermark is commonly modeled as a
`Frontier`, and a global watermark is commonly computed with `meet` across worker reports.

The terms are close but not interchangeable in every codebase. "Frontier" is the library's
general type and carries partial-order semantics. "Watermark" is the application meaning a
team gives that frontier, often with event-time and lateness rules attached. Use `Frontier`
when discussing the value structure and watermark when discussing the stream-processing
contract built on top of it.

## The public type vocabulary

### `Antichain<T>`

`Antichain<T>` is the minimal set of mutually incomparable values under `T`'s `PartialOrd`.
It is the data structure that preserves a multi-dimensional boundary without retaining values
that another element already dominates. Width zero and width one are stored inline, and a
fully ordered timestamp such as `u64` stays at width one, which keeps the common case small.

Use `Antichain<T>` when you need the boundary set itself. Most application code that is talking
about completed work will prefer `Frontier<T>`, because `Frontier` supplies the progress
interpretation and merge methods directly.

### `Frontier<T>`

`Frontier<T>` is a progress claim backed by an `Antichain<T>`. It is the type to reach for when
a worker, replica, partition, or operator needs to say how far it has safely advanced. It is a
pure value: it performs no networking, I/O, scheduling, locking, or persistence.

`Frontier::bottom()` creates the unconstrained frontier with no boundary elements. In the
frontier API it is the identity for `meet`, so it should not be casually conflated with every
other use of the word "bottom" in lattice terminology. The `WithBottom<T>` wrapper, described
below, adds an explicit domain value meaning "no progress yet"; that is a different semantic
choice.

### `Lattice`

`Lattice` is the trait that supplies `meet` and `join` for a value type. Implement it when the
domain has a coherent partial order and both bounds can be computed without hidden coordination.
The trait is the composition point for the crate: once a type implements it correctly, it can
be nested inside the other lattice types and inherit the same merge laws through those
compositions.

The trait is intentionally small. It does not define serialization, transport, membership,
writer ownership, or conflict policy. Those concerns may use a lattice value, but they belong
to the layer that understands the surrounding system.

### `ProductTimestamp<A, B>`

`ProductTimestamp<A, B>` models two independent dimensions. A value is below another only when
both components are below or equal. If one point is ahead on the first dimension and the other
is ahead on the second, the points are incomparable and can both remain in a frontier.

Use it for genuinely independent coordinates such as partition and offset. Do not use it merely
because a timestamp happens to be stored as a pair. If the first component dominates the second,
`Lexicographic` is the better expression of the domain.

### `Lexicographic<A, B>`

`Lexicographic<A, B>` models an outer dimension that dominates the inner dimension. The outer
value decides the ordering first; the inner value breaks ties within the same outer value. An
`(epoch, offset)` clock is the standard example: once the epoch changes, an offset from the old
epoch cannot outrank any offset in the new one.

This type usually keeps a frontier narrow because the order is total when its components are
appropriately ordered. It expresses hierarchy, not independence.

### `MapLattice<K, V>`

`MapLattice<K, V>` is a lattice over a dynamic set of named dimensions. It is the open-ended
counterpart to a fixed product: keys can appear as shards or partitions are added at runtime.
Join takes the union of keys and joins values for shared keys. Meet takes the intersection of
keys and meets values for shared keys, so only dimensions represented by both reports contribute
to the common claim.

The empty map represents no recorded dimensions and is the bottom of the map lattice. That is
useful when a topology is discovered incrementally, but it also means an empty map is absorbing
for meet. Applications should decide whether a missing key means "not started," "not observed,"
or "not a member" before choosing this representation.

### `SetLattice<T>`

`SetLattice<T>` uses set inclusion as its order. Meet is intersection and join is union. It is
useful when progress is about membership rather than a numeric position, such as the members
that have acknowledged a configuration or the replicas that have confirmed a fact.

As with `MapLattice`, the empty set is the bottom of the set lattice. It is the identity for
join and absorbing for meet. The meaning of membership must come from the application: a set of
acknowledgements, a set of completed partitions, and a set of observed peers may have identical
shape but different safety rules.

### `IntervalSetLattice<T>`

`IntervalSetLattice<T>` lives in the companion `antichain-intervals` crate and models progress
that arrives out of order with gaps. It stores a canonical set of disjoint, non-adjacent,
half-open intervals such as `[100, 150)` and `[200, 250)`. Insertions coalesce touching or
overlapping ranges, so the representation stays stable and equality means equal covered points.

Its order is set inclusion. Meet is intersection, which answers what two reports both cover;
join is union with coalescing, which answers what either report has covered. Use it for backfills,
block ingestion, or any workload where a single "highest offset" would conceal holes.

### `Max<T>` and `Min<T>`

`Max<T>` reverses the order of `T`, so a frontier meet computes the maximum underlying value.
It is useful for a lower-bound claim such as "every replica is at least at offset 500." `Min<T>`
preserves the natural order and makes the intent explicit when it is paired with `Max<T>` in a
composite value that tracks both lower and upper bounds.

These wrappers do not change the underlying number. They change how the number participates in
the order, which changes the meaning of meet and join. That makes them modeling tools, not
formatting conveniences.

### `Bounded<T>`

`Bounded<T>` stores a value together with an inclusive minimum and maximum, clamping values to
that interval at construction. The fixed range gives a predictable upper bound on how many
distinct incomparable values an antichain can contain. It is useful when the domain already has
a real finite range and that range matters to resource planning.

The bounds are part of the semantic contract. Mixing values with different ranges in one
antichain is not a meaningful operation, even if the Rust types permit it. Choose one range for
the domain and keep it consistent.

### `WithTop<T>` and `WithBottom<T>`

`WithTop<T>` adds a `Top` value above every ordinary value. It is useful for a stream or data
path that can be permanently sealed: `Top` is the identity for meet and absorbs join. Once a
closed path is represented as `Top`, it no longer constrains a conservative meet with a still
running path.

`WithBottom<T>` adds a `Bottom` value below every ordinary value. It makes "no progress yet" an
explicit state: `Bottom` absorbs meet and is the identity for join. The two wrappers can be
composed as `WithTop<WithBottom<T>>` when a domain needs both not-started and permanently-closed
states. These are values in the modeled order, not process lifecycle hooks; the application must
still decide when it is legitimate to publish either sentinel.

## System terms that sit around the crate

### CRDT

A CRDT, or conflict-free replicated data type, is a data structure whose replicas can merge
state without a central coordinator and converge on the same result. CRDTs usually model the
data itself: a set, counter, map, register, or another application value. Their merge is often
a join in a semilattice.

`antichain` shares the same algebraic foundation but focuses on progress rather than application
data. A system can use a CRDT to decide what data exists and a `Frontier` to decide how much of
that data is safe to read or process. Keeping those jobs separate prevents a progress boundary
from being mistaken for a conflict-resolution policy.

### Durable frontier

A durable frontier is a progress claim backed by the persistence guarantee the application
requires. A worker should publish it only after the writes represented by the claim have reached
the durable medium and can be recovered according to the system's failure model. The frontier
is therefore more than "the worker finished this loop iteration"; it is a statement about what
survives a crash.

Durability is not provided by `Frontier<T>`. The application decides when a claim becomes durable,
where it stores the claim, and how readers obtain it. `antichain` can merge the claims once those
rules exist.

### Safe read boundary

A safe read boundary is the frontier below which a reader is allowed to make a correctness claim.
A reader commonly computes it by taking the meet of durable frontiers from the participants that
must be included. The result is conservative: a slow or missing participant can hold the boundary
back, but the reader does not silently step past work that has not been certified by everyone in
scope.

A safe read boundary is not automatically a snapshot, a transaction, or a linearizable read.
Those stronger properties require rules about visibility, isolation, ownership, and atomicity
that live outside the merge value.

### Writer fencing, epoch, and compare-and-swap

Writer fencing is the mechanism that prevents an old or disconnected writer from publishing new
durable state after the system has moved on. An epoch, term, or generation identifies the
currently valid writer incarnation. Compare-and-swap, often abbreviated CAS, lets a writer
publish a change only when the shared record still contains the version it read.

These mechanisms are deliberately outside `antichain`. A frontier can merge a stale writer's
claim perfectly and still be wrong if the writer was no longer authorized to produce the claim.
When a design uses `antichain` in a storage system, fencing is part of the correctness boundary,
not an optional operational improvement.

### Transport and gossip

Transport is how progress claims move between participants. Gossip is one transport pattern:
each participant exchanges the values it knows with some peers, and repeated exchange spreads
the facts through the system. A manifest, message queue, RPC call, or object-store record can
serve the same role.

The merge laws make delayed, reordered, duplicated, and retried delivery safe, but they do not
make dropped information disappear. Convergence requires that the participants whose answers are
being compared eventually learn the same relevant facts, or that the application has an explicit
rule for membership and omission.

### Eventual consistency and linearizability

Eventual consistency means that replicas may temporarily return different answers while updates
are in flight, but replicas that receive the same updates eventually agree. This is the kind of
convergence a mergeable progress value naturally supports. A conservative frontier can improve
safety by refusing to advance past uncertain work, but it does not turn an eventually consistent
transport into a linearizable storage service.

Linearizability requires each operation to appear to take effect at one point between its call
and return, with all clients observing an order consistent with real time. If an application
needs that guarantee, it needs a protocol for total ordering, ownership, or consensus in addition
to the lattice merge.

## Language we keep precise

When we say **safe**, we mean that the claim is supported by the participants and durability
rules included in the protocol. We do not mean that the value is merely conservative-looking.
When we say **global**, we mean global over an explicit participant set; a frontier cannot speak
for a writer or shard it was never required to observe. When we say **converged**, we mean that
the relevant nodes have incorporated the same facts and therefore compute the same merge result.

When discussing a design, name the order and the scope alongside the value. "The frontier is
7" is incomplete. "The `u64` frontier for the three durable writers is 7, so timestamps below 7
are safe for the current read scope" tells the reader what the number means, who it covers, and
what it does not certify.

## Related reading

- [README](README.md) for the short project overview and public type catalogue.
- [FAQ](docs/faq.md) for gentle explanations of frontiers, lattice laws, and convergence.
- [Cookbook](docs/cookbook.md) for problem-to-type recipes with compilable examples.
- [Prior art and positioning](docs/comparison.md) for the relationship to CRDTs and
  timely-dataflow.
- [SlateDB multi-writer design](docs/slatedb-multi-writer.md) for an application-level narrative
  using frontiers, CRDTs, durable progress, and writer fencing together.
