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

It is a small Rust library for tracking *how far along* a job is across many computers —
without needing a central "boss" computer to keep score.

### 2. What problem does it actually solve?

Imagine lots of workers chewing through a big pile of work. Every so often something needs
to ask: *"Is it safe to act now? Has everyone gotten far enough?"* Normally you'd put one
machine in charge of collecting everyone's progress and announcing the answer. `antichain`
lets the workers figure out the answer **among themselves**, by combining their progress
reports directly, so you don't need that central machine.

### 3. Why is avoiding a central coordinator a big deal?

A central coordinator is a **bottleneck** (everything funnels through it), a **single point
of failure** (if it dies, nobody knows the global progress), and a **consistency hazard**
(while it's half-way through updating, it can report a wrong answer). Removing it removes
all three problems at once.

### 4. Can you give me a real-world analogy?

Think of a group of hikers spread along a trail. You want to know "where is the *slowest*
hiker, so we know everyone is at least that far?" One way: everyone radios a leader who
tracks the minimum. `antichain`'s way: any two hikers who meet on the trail compare notes
and remember the more-conservative (further-back) position. After enough chance meetings,
*every* hiker independently knows the true slowest position — no leader required, and it
doesn't matter who bumped into whom or in what order.

### 5. Who is this library for?

People building distributed or parallel systems: stream processors, databases, replication
systems, backfill/ingestion pipelines, and anything that needs to answer *"is everyone past
point X yet?"* across multiple workers or machines.

### 6. Do I need to be a mathematician to use it?

No. The library is designed so you pick a type that matches your problem and call `meet`.
The maths is there to *guarantee correctness*, but you don't have to understand it to
benefit from it. The [Tutorial](tutorial.md) and [Cookbook](cookbook.md) are written for
working engineers.

### 7. Is this a database, a message queue, or a networking library?

None of those. It is a **pure data type** — a building block. It does no networking, no
disk I/O, no threads. You feed it progress values and it tells you how they combine. You
build the networking/storage around it however you like.

### 8. What does the name "antichain" mean?

It comes from order theory. An **antichain** is a set of things where no item is "ahead of"
or "behind" any other — they are all mutually incomparable. That's exactly what you need to
represent multi-dimensional progress where different workers are ahead in different ways.
(See [§24](#24-what-is-an-antichain-precisely) for the precise definition.)

### 9. Is it production-ready?

The core is small, fully property-tested, formally model-checked for its central
convergence guarantee, benchmarked, and published on crates.io. It is `1`-idea-sharp by
design. As with any `0.x` crate, review the [CHANGELOG](../CHANGELOG.md) and pin a version.

### 10. What's the quickest way to see it work?

Two lines:

```rust
use antichain::Frontier;
let global = Frontier::from_elem(10u64).meet(&Frontier::from_elem(7u64));
assert_eq!(global, Frontier::from_elem(7u64)); // the more conservative bound wins
```

Or run a live demo: `cargo run --example watermark_gossip`.

---

## Core ideas in plain words

### 11. What is a "frontier"?

A **frontier** is a progress claim: *"everything below this line is finished."* If a worker
says its frontier is `7`, it means *"I've completed everything up to 7."* It's the same idea
as a **watermark** in stream processing.

### 12. What is "merging" two frontiers?

It is combining two progress claims into the single claim that is safely true for *both*.
If worker A is done up to 10 and worker B is done up to 7, the only thing safely true for
both is "done up to 7." That combine step is called **`meet`**.

### 13. What is `meet`?

`meet` is the core operation of the whole crate: the **coordinator-free merge**. For plain
numbers it computes the *minimum* — the most conservative, safe-for-everyone answer. For
richer types it computes the equivalent "greatest lower bound." It is the one function you
will call most.

### 14. What is `join`, then?

`join` is the opposite: the *least upper bound* — the most optimistic combination ("the
furthest anyone has reached"). For plain numbers it's the *maximum*. You use `join` to
**advance** progress; you use `meet` to find the **safe shared** progress.

### 15. Why are `meet` and `join` weird names? Why not `min`/`max`?

Because the library works for many kinds of "progress," not just numbers. For two-dimensional
progress, or sets, or maps, "minimum" doesn't quite mean anything — but "greatest lower
bound" (`meet`) always does. The names come from **lattice theory**, where they have a
precise meaning across all these types.

### 16. What makes the merge safe to do in any order?

Three properties of `meet`:

- **Commutative** — `meet(a, b) == meet(b, a)`: order of the two inputs doesn't matter.
- **Associative** — `meet(a, meet(b, c)) == meet(meet(a, b), c)`: grouping doesn't matter.
- **Idempotent** — `meet(a, a) == a`: merging the same thing twice changes nothing.

Together these mean messages can arrive **late, out of order, or duplicated**, and everyone
still ends up with the identical, correct answer.

### 17. Why does idempotence matter so much in practice?

Networks redeliver messages. If merging a duplicate progress report could change your answer,
you'd need exactly-once delivery — which is hard and expensive. Because `meet` is idempotent,
duplicates are simply harmless. You can use cheap at-least-once delivery (gossip, retries).

### 18. What is "convergence"?

Convergence is the promise that **two nodes that have seen the same set of updates — in any
order, with any duplicates — end up with exactly the same frontier.** It is the formal reason
you can delete the coordinator and still trust the answer. (See [§52](#52-is-the-convergence-guarantee-actually-proven).)

### 19. Is this the same idea as CRDTs?

Yes — the same algebra. A CRDT merges replicated **data** without coordination; `antichain`
merges **progress** without coordination. Same mathematical trick (a semilattice merge),
applied to a different thing. (See [§59](#59-how-is-this-different-from-a-crdt-library).)

### 20. What's the difference between `Antichain`, `Frontier`, and `Lattice`?

- **`Lattice`** is a *trait* (an interface): "this type knows how to `meet` and `join`."
- **`Antichain<T>`** is a *set* of mutually-incomparable values, kept minimal automatically.
- **`Frontier<T>`** is a *progress claim* backed by an `Antichain<T>`. It's the type you
  usually hold and merge.

### 21. When would a frontier hold more than one value?

When progress is genuinely multi-dimensional and two states are *incomparable* — neither is
"ahead." Example: worker A finished partition 0 but not 1; worker B finished partition 1 but
not 0. Neither dominates, so the frontier keeps **both** as the honest description of "the
boundary of what's done."

### 22. If I only ever track a single number per worker, is this overkill?

Not at all — that's the simplest and fastest case. A `Frontier<u64>` collapses to a single
value, merges in constant time, and never touches the heap. You get the coordinator-free
guarantees with essentially zero overhead.

### 23. What does "coordinator-free" *not* mean?

It does **not** mean "no networking needed" or "magically consistent data." You still have
to ship the progress values between nodes somehow. What you get for free is that *however*
you ship them — any order, any duplication — the merged result is always correct.

---

## The maths, gently

*You can use the whole library without this section. It's here for the curious and for
people who want to understand* why *the guarantees hold.*

### 24. What is an antichain, precisely?

Given a partial order (a notion of "≤" that not every pair has to satisfy), an **antichain**
is a subset in which no two distinct elements are comparable: for any `x ≠ y` in the set,
neither `x ≤ y` nor `y ≤ x`. It is the set of "tied / incomparable" frontier points.

### 25. What is a partial order, in everyday terms?

It's a way of saying "x is at least as far as y" that allows for **ties of incomparability**.
With ordinary numbers, any two are comparable (a *total* order). With, say, (partition,
offset) pairs, some pairs simply can't be ranked — that's a *partial* order. The "partial"
just means "some pairs are incomparable."

### 26. What is a lattice?

A partially ordered set where every pair of elements has both a **greatest lower bound**
(their `meet`) and a **least upper bound** (their `join`). Intuitively: any two states have
a well-defined "most conservative common ground" and "most optimistic combination."

### 27. What is a "greatest lower bound"?

The largest value that is still ≤ both inputs. For numbers `5` and `8`, lower bounds are
`5, 4, 3, …`; the *greatest* of them is `5`. So `meet(5, 8) = 5 = min`. The phrasing
generalizes to types where "min" has no meaning.

### 28. What is a semilattice, and why do I keep seeing "semi"?

A **semilattice** is a set with just *one* of the two operations (say, only `meet`) that is
commutative, associative, and idempotent. The convergence guarantee only needs *one*
direction, so the merge side of `antichain` is really a **meet-semilattice**. The full
`Lattice` trait provides both directions for convenience.

### 29. Why do commutativity, associativity, and idempotence delete the coordinator?

Because they make the *final result independent of the schedule*. A coordinator exists to
impose an order ("collect everyone, then compute"). If the answer doesn't depend on order,
grouping, or repetition, there is nothing left for a coordinator to decide — any node can
combine whatever it has heard and reach the same place.

### 30. What is "absorption" / domination?

If state `a` is "ahead of or equal to" `b` (i.e. `b ≤ a`), then merging them by `join` just
gives `a` back — `a` *absorbs* `b`. In an antichain, a newly inserted element that dominates
an existing one **replaces** it; one that is dominated is **dropped**. This is what keeps the
antichain minimal.

### 31. What is the "consistency law" the docs mention?

A single biconditional that ties the order and the operations together:

$$a \le b \iff \mathrm{meet}(a, b) = a \iff \mathrm{join}(a, b) = b.$$

Every public type in the crate is property-tested to satisfy it in **both** directions. It's
the law most likely to expose a subtle bug in a hand-written lattice, so it's tested
explicitly.

### 32. Why is `meet` the "safe" choice and `join` the "optimistic" one?

`meet` moves *down* toward the bottom (the most conservative "everyone agrees" point), so
acting below it is always safe. `join` moves *up* toward the top (the furthest anyone has
seen), which is useful for advancing your own knowledge but is *not* a guarantee that
everyone is there yet.

### 33. What is the "bottom" element?

`⊥` ("bottom") is the least element — *"no progress yet; nothing is complete."* For a
`Frontier`, `Frontier::bottom()` is the empty frontier. It is the identity for `meet`-based
accumulation's counterpart `join`, and the natural starting point before any updates.

### 34. What is the "top" element?

`⊤` ("top") is the greatest element — *"completely done / sealed."* Not every type has one
built in, which is exactly why `WithTop<T>` exists: it adds a structural `Top` so you can
represent "this stream is permanently closed" without abusing a magic number like `u64::MAX`.

### 35. What's the minimal contract a type `T` must satisfy to be used?

For `Antichain`/`Frontier`, `T` needs `PartialOrd + Clone`. To get `meet`/`join` at the value
level, `T` must implement the crate's `Lattice` trait. The property tests document the exact
algebraic laws a correct `Lattice` impl must obey.

### 36. Does the partial order have to be a total order?

No — that's the entire point. Totally ordered types (`u64`) are the easy special case where
the antichain always has width 1. Partially ordered types are where frontiers earn their
keep by holding several incomparable points at once.

### 37. Is there a formal definition of the convergence theorem?

Yes: *"If two nodes have each observed any subset of the same update set, in any order, their
`Frontier` values are identical after merging all updates."* It is stated in the README and
mechanically checked by the Fizzbee spec at
[`specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz).

---

## For software engineers: using the library

### 38. How do I add it to my project?

```toml
[dependencies]
antichain = "0.3"
```

With serialization: `antichain = { version = "0.3", features = ["serde"] }`.
For `no_std`: `antichain = { version = "0.3", default-features = false }`.

### 39. What's the smallest useful example?

```rust
use antichain::Frontier;

let a = Frontier::from_elem(120u64);
let b = Frontier::from_elem(95u64);
let global = a.meet(&b);               // most conservative shared progress

assert!(global.less_equal(&95));       // 95 may still be in-flight
assert_eq!(a.meet(&b), b.meet(&a));    // order never matters
```

### 40. How do I build a frontier?

- `Frontier::bottom()` — empty ("no progress").
- `Frontier::from_elem(t)` — a single progress point.
- `Frontier::from_elements(iter)` — from many points; it keeps only the minimal
  (incomparable) ones automatically.

### 41. How do I merge many frontiers at once?

Fold `meet` over them; order and grouping don't matter:

```rust
use antichain::Frontier;
let workers = [120u64, 95, 200, 88].map(Frontier::from_elem);
let global = workers.iter().cloned().reduce(|a, b| a.meet(&b)).unwrap();
assert_eq!(global, Frontier::from_elem(88u64));
```

### 42. What does `less_equal` tell me?

`frontier.less_equal(&t)` answers *"is `t` at or below the frontier boundary?"* — i.e. has
the frontier reached/covered `t`. It returns `true` when `t ≤ some element` of the antichain.
It's how you ask "has progress passed this timestamp?"

### 43. How do I read what's inside a frontier?

`frontier.elements()` returns the slice of antichain points. For a `Frontier<u64>` that's a
single value; for a multi-dimensional frontier it's the set of incomparable boundary points.

### 44. When do I use `Antichain<T>` directly instead of `Frontier<T>`?

Most of the time, use `Frontier`. Reach for `Antichain<T>` when you want the raw
invariant-maintaining set behaviour — e.g. to build a custom structure — using `empty()`,
`from_elem()`, `insert()`, `elements()`, `len()`, `is_empty()`, and `less_equal()`.

### 45. What does `Antichain::insert` do with the invariant?

It keeps the set minimal automatically: inserting an element that is dominated by an existing
one is a no-op; inserting one that dominates existing elements removes them. You never have to
de-duplicate or prune by hand.

### 46. How do I represent two independent dimensions?

Use `ProductTimestamp<A, B>` (the **product order**), *not* a plain tuple. Standard tuples
compare lexicographically, which is a different order.

```rust
use antichain::{Frontier, ProductTimestamp};
type Pt = ProductTimestamp<u64, u64>;          // (partition, offset)
let merged = Frontier::from_elem(Pt::new(1, 50)).meet(&Frontier::from_elem(Pt::new(2, 30)));
assert_eq!(merged.elements().len(), 2);        // incomparable → both kept
```

### 47. What if the outer dimension should dominate the inner one?

Use `Lexicographic<A, B>`: once the outer value (e.g. an epoch) advances, the inner counter
is irrelevant. This keeps the antichain at width 1.

```rust
use antichain::{Frontier, Lexicographic};
type Clock = Lexicographic<u64, u64>;          // (epoch, offset)
let merged = Frontier::from_elem(Clock::new(4, 900)).meet(&Frontier::from_elem(Clock::new(5, 10)));
assert_eq!(merged.elements(), &[Clock::new(4, 900)]); // epoch 4 < epoch 5 outright
```

### 48. My set of workers/shards changes at runtime. What do I use?

`MapLattice<K, V>` — a per-key lattice backed by a `BTreeMap`. Each shard appears the moment
it first reports. `meet` = key **intersection** + value-meet ("what everyone agrees on");
`join` = key **union** + value-join ("the furthest anyone has reached").

### 49. How do I track which discrete members have acknowledged?

`SetLattice<T>` (partial order = subset inclusion). `meet` = intersection ("acknowledged by
everyone"); `join` = union ("acknowledged anywhere"). Perfect for quorum/ack tracking.

### 50. I want "everyone is *at least* here," not "at most here." How?

Wrap the value in `Max<T>` to invert the order, so `meet` keeps the **higher** value (a
guaranteed floor). Pair `Max<T>` with `Min<T>` in a tuple to carry a lower *and* upper bound
through one frontier.

### 51. How do I signal "this stream is permanently closed" or "not started"?

`WithTop<T>` adds a `Top` sentinel (closed/sealed/EOF): `Top` absorbs `join` and is the
identity for `meet`. `WithBottom<T>` adds a `Bottom` sentinel (not started): `Bottom` absorbs
`meet` and is the identity for `join`. Compose as `WithTop<WithBottom<T>>` for a fully closed
`Bottom < Value(t) < Top` lattice — no magic constants.

### 52. How do I track out-of-order ranges with gaps (e.g. backfill)?

Use `IntervalSetLattice<T>` from the companion crate
[`antichain-intervals`](../crates/antichain-intervals). It keeps a canonical set of disjoint
intervals; `join` coalesces overlapping ranges, `meet` intersects them. Ideal when block 150
arrives before block 101.

### 53. Can I clamp a value to a finite range?

Yes — `Bounded<T>` clamps to `[min, max]` at construction (out-of-range inputs are clamped,
never rejected). Because the range is finite, the antichain width is provably bounded by the
range's cardinality. Keep all values in one antichain on the **same** range.

### 54. Do I have to handle de-duplication of messages myself?

No. Because `meet` is idempotent, re-merging a value you've already seen has no effect. You
can safely use at-least-once delivery without tracking what you've already applied.

### 55. Is `Frontier` cheap to clone?

For totally-ordered types (`Frontier<u64>`) it's allocation-free, so cloning is trivial. For
genuinely partially-ordered frontiers of width ≥ 2 it clones a small `Vec`. Width stays small
in practice (typically ≤ ~50). (See [§61](#61-how-fast-is-meet).)

### 56. Is the API stable? Will my code break?

The crate follows semver and runs `cargo-semver-checks` in CI. Within `0.3.x` the public API
is stable. Breaking changes bump the minor version (per `0.x` semver) and are recorded in the
[CHANGELOG](../CHANGELOG.md).

---

## Choosing and composing types

### 57. There are a lot of types. How do I pick one?

Use the decision table in the **[Cookbook](cookbook.md#decision-table--which-type-do-i-use)**.
The short version:

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

### 58. Can I combine these types?

Yes — composition is the whole point. The composite's partial order (and the convergence
guarantee) are derived automatically from the parts. Examples:
`Frontier<(Max<u64>, Min<u64>)>`, `MapLattice<ShardId, ProductTimestamp<u64, u64>>`,
`WithTop<WithBottom<u64>>`.

### 59. Why shouldn't I just use a plain tuple `(A, B)`?

A standard-library tuple compares **lexicographically**, not by the **product order**. For
*independent* dimensions that's the wrong order — and component-wise `meet` on a lexicographic
tuple is not a true greatest lower bound. Use `ProductTimestamp` for independent axes;
`Lexicographic` when you really do want the outer to dominate.

### 60. When should I use `MapLattice` instead of widening a `Frontier`?

When the number of dimensions is dynamic (shards come and go), or when an antichain would
otherwise grow large because each "dimension" is really a separate keyed channel. Keying by a
`MapLattice` keeps each value totally-ordered and the structure clear.

### 61. What's the difference between `Max<T>` and `Min<T>` again?

`Max<T>` **inverts** the order so `meet` computes `max` (a guaranteed lower bound / floor).
`Min<T>` is a transparent newtype that keeps the natural order; its value is documentary —
it pairs cleanly with `Max<T>` in composites like `(Max<u64>, Min<u64>)`.

### 62. Why does `Bounded<T>` need all values to share the same range?

Its lattice operations use `self`'s bounds, so mixing different `[min, max]` ranges in one
antichain is undefined by design. Decide the range once and use it consistently across the
frontier.

---

## Performance and internals

### 63. How fast is `meet`?

For totally-ordered types (`Frontier<u64>`) it's effectively **O(1)** and allocation-free —
the antichain collapses to width 1. For partially-ordered types it's **O(n·m)** in the two
antichain widths. Measured on an Apple M-series, release build:

| Operation | Width 10 | Width 100 | Width 1000 |
|-----------|----------|-----------|------------|
| `Frontier<ProductTimestamp>::meet` | 147 ns | 9.2 µs | 825 µs |
| `Frontier<u64>::meet` (width 1) | 18 ns | 18 ns | 18 ns |

### 64. Will the antichain "explode" in size for multi-dimensional progress?

In practice, no. Empirically, widths stay ≤ ~50 for real workloads, where `meet` costs under
a microsecond. If a dimension can grow unbounded, model it as a `MapLattice` key instead of
widening the antichain. This was the single highest-risk question in the roadmap and was
closed with benchmark data, not assumption.

### 65. Does it allocate memory?

Width-0 and width-1 antichains are stored **inline with zero heap allocation**. Only
genuinely partially-ordered antichains of width ≥ 2 spill to a `Vec`. A shrinking antichain
renormalizes back down to the allocation-free representation.

### 66. Is there a compaction step on `meet`?

No — and benchmarks showed one isn't needed. Width ≤ 100 costs under 10 µs; beyond that you've
exceeded practical system widths. The recommended fix for genuine width growth is structural
(use `MapLattice`), not a runtime compaction pass.

### 67. Is it `no_std` compatible?

Yes. Disable the default `std` feature; only `alloc` (a global allocator) is required. Both
the core crate and `antichain-intervals` support `no_std`, and CI builds that configuration.

### 68. Does it use any `unsafe` code?

No. Both crates carry `#![forbid(unsafe_code)]`, so there is provably no `unsafe` anywhere in
the source.

### 69. What are the runtime dependencies?

Effectively none for the core data type. `serde` is optional and feature-gated;
`proptest`/`criterion`/`serde_json` are dev-only (tests and benchmarks). This keeps it a
boring, portable primitive.

### 70. How big is the codebase?

Small and auditable by design — a single-file core plus the companion intervals crate. The
philosophy is "the math is where the certainty lives": keep the primitive tiny and prove it,
rather than growing a large surface.

---

## Correctness, testing, and formal proofs

### 71. How do I know the algebra is actually correct?

Every public type is **property-tested** over 10 000+ random cases for commutativity,
associativity, idempotence, absorption, the antichain invariant, and the universal
consistency law `a ≤ b ⟺ meet(a,b)==a ⟺ join(a,b)==b` — in both directions.

### 72. Is the convergence guarantee actually proven?

Yes, mechanically. A Fizzbee model-checking spec
([`specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz)) exhaustively
enumerates *every* interleaving of update deliveries across nodes and asserts convergence in
every reachable state — so no adversarial ordering can cause divergence.

### 73. How do I run the formal check myself?

```sh
brew tap fizzbee-io/fizzbee && brew install fizzbee
fizz specs/frontier_convergence.fizz
```

### 74. What's the difference between the property tests and the formal spec?

Property tests throw thousands of *random* inputs at the real Rust code. The formal spec
*exhaustively* explores all message orderings of an abstract model. Together they cover both
"the implementation behaves" and "the design is sound under every schedule."

### 75. Is the library fuzzed?

Yes — there are `cargo-fuzz` targets exercising the `insert` and `meet` paths (in `fuzz/`),
to catch panics or invariant violations on adversarial inputs.

### 76. Are the documentation examples tested?

Yes. The Cookbook and Tutorial code blocks are compiled and run as doctests (via a
`#[cfg(doctest)]` include), so the docs cannot silently rot out of sync with the code.

### 77. What does CI enforce?

`cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -D warnings`, the
full test suite (including `--no-default-features` for `no_std`), an MSRV `cargo check`, and
`cargo-semver-checks`. Test code must be lint-clean too.

---

## How it compares to other tools

### 78. How does this differ from timely-dataflow / differential-dataflow?

Those are the direct intellectual ancestors and implement the same `Antichain`/`Frontier`
algebra — but as part of a **full dataflow runtime** (scheduling, communication, workers).
`antichain` extracts *just the progress primitive*: zero runtime dependencies, `no_std`, a
broader composition toolkit, and a formally model-checked convergence spec. Use timely if you
want the whole engine; use `antichain` if you want the primitive in isolation.

### 79. How is this different from a CRDT library?

Same algebra, different target. CRDTs replicate application **data** (counters, sets, maps)
and usually merge by `join`. `antichain` tracks **progress** ("how far has computation got")
and usually merges by `meet`. A common pattern is to combine them: a CRDT for the data, a
`Frontier` for the fence that says when the data is safe to read.

### 80. Isn't an `Antichain` just a priority queue or a sorted set?

No. A priority queue keeps a *total* order and surfaces a single min/max. A sorted set orders
everything. An antichain keeps the **Pareto frontier** of a *partial* order — precisely the
mutually-incomparable elements — and its invariant-maintaining `insert` is the key difference.

### 81. When should I *not* use this crate?

When your problem is **ownership/membership/consensus** ("who is allowed to write to shard
42?", "what happens when a node crashes?"). That's a different problem class needing leases or
Raft. `antichain` tracks *progress*, not *authority*, and deliberately refuses to pretend
otherwise.

### 82. Can I use it alongside Raft / a consensus system?

Absolutely — they're complementary. Use consensus for the control plane (membership, leader
election, ownership) and `antichain` for the data-plane progress tracking that doesn't need a
coordinator. Keeping that seam sharp is a core design principle.

### 83. Where can I read a fuller comparison?

See [`docs/comparison.md`](comparison.md) for a fair, strawman-free comparison to
timely/differential-dataflow and CRDT libraries, including what each is better at.

---

## Project, packaging, and practical matters

### 84. What version is current?

`antichain` `0.3.0` and the companion `antichain-intervals` `0.1.0`. See the
[CHANGELOG](../CHANGELOG.md) for the full history back to `0.1.0`.

### 85. What is the Minimum Supported Rust Version (MSRV)?

Rust `1.85` (edition 2024). CI runs a dedicated MSRV `cargo check` job so the policy can't
silently regress.

### 86. What license is it under?

Apache-2.0.

### 87. How do I enable serialization?

Turn on the `serde` feature: `antichain = { version = "0.3", features = ["serde"] }`. The
wire format for `Antichain`/`Frontier` is `{ "elements": [...] }`, locked by round-trip tests
so it won't change underneath you.

### 88. Does serde work in `no_std`?

Yes — the `serde` feature pulls in serde's `alloc` collection support, so it composes with a
`no_std` + `alloc` build. (An earlier release had a latent bug here; it's fixed and guarded by
tests.)

### 89. Why is there a separate `antichain-intervals` crate?

`IntervalSetLattice<T>` needs a non-trivial interval-coalescing data structure. Keeping it in
a companion crate keeps the core lean, dependency-light, and `no_std`-simple, while still
implementing `antichain::Lattice` so it drops straight into a `Frontier` or `MapLattice`.

### 90. Where are the runnable examples?

In [`examples/`](../examples):

- `watermark_gossip.rs` — N simulated workers gossiping over a lossy channel, converging to a
  shared watermark (a live demonstration of the convergence theorem).
- `backfill_gaps.rs` — out-of-order block arrival with `antichain-intervals`.
- `progress_protocol.rs` — a complete three-layer Worker → Shard → Cluster protocol built
  entirely on the public API.

Run one with `cargo run --example watermark_gossip`.

### 91. Where's the API reference?

On [docs.rs/antichain](https://docs.rs/antichain). Every public item has rustdoc with
examples and the relevant law explanations inline.

### 92. How do I learn it properly, in order?

1. This FAQ (the first three sections).
2. The **[Tutorial](tutorial.md)** — a narrative from "one number" to a frontier.
3. The **[Cookbook](cookbook.md)** — pick-a-type recipes.
4. The **[Design notes](idea.md)** — the algebra and the boundaries.
5. The **[API docs](https://docs.rs/antichain)** — the reference.

### 93. Can I contribute? What's the review bar?

Yes. The bar is high on correctness: new lattice types must come with the full property-test
suite (including the consistency law) and fit the "progress only, no consensus" scope. The
[roadmap](../roadmap.md) records how the crate was built phase by phase and what's deferred.

### 94. What's explicitly *out of scope* for this crate?

Networking/gossip protocols, consensus/leader-election/leases, storage engines, and query
planners. Those are legitimate things to *build on top of* the primitive — they are not the
primitive. Keeping that boundary sharp is the project's central discipline.

### 95. Is there a roadmap of what's next?

Yes — [`roadmap.md`](../roadmap.md). Phases 0–10 (core, proofs, composition, hardening,
formal spec, extended lattices, performance, adoption docs, onboarding) are complete.
Future work would be additional composition patterns *only if* real downstream usage proves a
genuine need.

---

## Troubleshooting and common gotchas

### 96. My two-dimensional frontier has two elements and I expected one. Bug?

Almost certainly not. If the two points are genuinely incomparable under the product order
(neither dominates), keeping both is the *correct* behaviour — that's the honest boundary.
If you actually wanted the outer dimension to dominate, you want `Lexicographic`, not
`ProductTimestamp`.

### 97. I used a tuple and the merge result looks wrong.

A plain `(A, B)` tuple uses **lexicographic** comparison, and component-wise `meet` on it is
*not* a true greatest lower bound — so it's deliberately excluded from the universal
consistency law. Use `ProductTimestamp<A, B>` for independent dimensions.

### 98. `meet` gave me a *smaller* number than both inputs with `Max<T>` — wait, no, a larger one.

That's correct: `Max<T>` inverts the order, so `meet` on `Max` values computes the **maximum**.
If you wanted the minimum, use the bare value (or `Min<T>`). The wrapper's whole job is to flip
which direction "safe" points.

### 99. My `MapLattice` `meet` dropped keys I expected to keep.

`meet` on a `MapLattice` is a **key intersection** — only keys present in *both* maps survive
(with their values met). That's the conservative "what everyone agrees on" answer. If you
wanted every key, you want `join` (key union).

### 100. `--features serde` won't compile in my older setup.

Make sure you're on `0.3.x`; an earlier release didn't wire serde's `alloc` impls correctly
for `MapLattice`/`SetLattice`. Upgrading fixes it. If you're on `no_std`, remember serde needs
its `alloc` support, which the feature now enables for you.

### 101. I'm getting a width-explosion in a high-dimensional frontier.

Don't widen a single antichain across an unbounded dimension. Move that dimension into a
`MapLattice` key (so each value stays totally-ordered), or, if it's a finite range, wrap it in
`Bounded<T>` to cap the width by the range's cardinality.

### 102. Why does `Frontier::bottom()` say nothing is complete, but it's the starting point?

`bottom()` is `⊥` — *"no progress yet."* It's the honest starting state before any updates and
the identity you fold `join` from. Don't confuse "bottom of the lattice" (least progress) with
"the answer" — you build *up* from bottom as workers report.

### 103. Is `less_equal` asking "is the frontier past t" or "is t past the frontier"?

`frontier.less_equal(&t)` is `true` when `t ≤ some frontier element` — i.e. the frontier has
**reached or covered** `t`. Read it as *"has progress arrived at `t`?"* `false` means `t` is
strictly beyond the frontier (not yet reached).

### 104. Where do I report a bug or ask a question not covered here?

Open an issue on the GitHub repository. If it's a correctness concern, a failing `proptest`
seed or a minimal reproduction is the most useful thing you can attach.

---

*Didn't find your question? The [Tutorial](tutorial.md) builds the intuition from scratch, the
[Cookbook](cookbook.md) maps problems to types, and [`idea.md`](idea.md) explains the
philosophy and scope.*
