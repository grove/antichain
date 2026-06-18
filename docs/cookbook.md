# Antichain Cookbook

A task-oriented guide to modeling distributed progress with `antichain`.

The API reference on [docs.rs](https://docs.rs/antichain) tells you *what each type does*.
This cookbook answers the more practical question:

> **"I have problem X — which type do I reach for, and how do I wire it up?"**

New to the crate or have a more general question? The **[FAQ](faq.md)** answers 100+ questions,
starting from the very basics.

Every snippet here is a complete, compilable example. If you want the algebraic
motivation behind the design, read [`idea.md`](idea.md); if you want the formal
convergence proof, see [`../specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz).

---

## The one rule to remember

`meet` is the **coordinator-free merge**. It is commutative, associative, and
idempotent, so nodes can exchange progress in any order, over any network, with
duplicates or reordering, and still converge to the same answer.

Everything in this crate exists to let you pick the *right partial order* for your
domain so that `meet` computes the answer you actually want.

---

## Decision table — which type do I use?

| Your situation | Reach for | Section |
|----------------|-----------|---------|
| A single monotonic clock / watermark / offset | `Frontier<u64>` | [§1](#1-a-single-watermark) |
| Two **independent** clocks (e.g. partition × offset) | `Frontier<ProductTimestamp<A, B>>` | [§2](#2-two-independent-dimensions) |
| Outer clock dominates, inner breaks ties (epoch × offset) | `Frontier<Lexicographic<A, B>>` | [§3](#3-an-epoch-that-dominates-an-offset) |
| The **set of dimensions changes at runtime** (shards added/removed) | `MapLattice<K, V>` | [§4](#4-a-cluster-that-rescales-at-runtime) |
| Track which discrete members have acknowledged | `SetLattice<T>` | [§5](#5-quorum--acknowledgement-sets) |
| A "lower bound" that should merge by `max`, not `min` | `Max<T>` | [§6](#6-tracking-a-lower-bound-instead-of-an-upper-bound) |
| A lower **and** upper bound tracked together | `(Max<T>, Min<T>)` | [§6](#6-tracking-a-lower-bound-instead-of-an-upper-bound) |
| A value confined to a finite `[min, max]` range | `Bounded<T>` | [§7](#7-bounding-antichain-width-with-a-finite-range) |
| A stream that can permanently **close** (EOF / sealed) | `WithTop<T>` | [§8](#8-signalling-a-permanently-closed-stream) |
| A path that may have **not started yet** (explicit "no progress") | `WithBottom<T>` | [§8](#8-signalling-a-permanently-closed-stream) |

If two situations apply, **compose the types** — that is the whole point. See
[§9](#9-composing-everything).

---

## 1. A single watermark

The simplest case: one monotonically advancing integer (a Kafka offset, a log
sequence number, an event-time watermark in milliseconds).

```rust
use antichain::Frontier;

// Two workers report progress independently.
let worker_a = Frontier::from_elem(120u64);
let worker_b = Frontier::from_elem(95u64);

// The safe global frontier is the meet — the most conservative bound.
let global = worker_a.meet(&worker_b);

assert!(global.less_equal(&95));   // timestamp 95 may still be in-flight
assert!(!global.less_equal(&120)); // everything below 120 is NOT yet globally safe

// Order never matters.
assert_eq!(worker_a.meet(&worker_b), worker_b.meet(&worker_a));
```

For a totally-ordered type like `u64`, the antichain always collapses to a single
element (the minimum), so `meet` is effectively **O(1)** no matter how many updates
you fold in.

---

## 2. Two independent dimensions

When progress has two axes that advance *independently* — say a partition index and
a byte offset — neither one dominates the other. Use `ProductTimestamp`, **not** a
plain tuple. (Standard-library tuples compare lexicographically, which is a different
order; see [§3](#3-an-epoch-that-dominates-an-offset).)

```rust
use antichain::{Frontier, ProductTimestamp};

type Pt = ProductTimestamp<u64, u64>;

// (partition, offset) pairs that are genuinely incomparable.
let a = Frontier::from_elem(Pt::new(1, 50));
let b = Frontier::from_elem(Pt::new(2, 30));

let merged = a.meet(&b);

// Neither pair dominates the other, so BOTH survive in the antichain.
assert_eq!(merged.elements().len(), 2);
```

Because incomparable elements accumulate, the antichain can grow to *width n*. In
practice this stays small (≤ ~50); if it grows unbounded, model each dimension as a
`MapLattice` key instead (see [§4](#4-a-cluster-that-rescales-at-runtime)).

---

## 3. An epoch that dominates an offset

Sometimes the outer dimension *totally* dominates: once the epoch advances, the inner
offset resets and is irrelevant for comparison. That is **lexicographic** order, and
it keeps the antichain at width 1.

```rust
use antichain::{Frontier, Lexicographic};

type Clock = Lexicographic<u64, u64>; // (epoch, offset)

let a = Frontier::from_elem(Clock::new(4, 900));
let b = Frontier::from_elem(Clock::new(5, 10));

// Epoch 5 dominates epoch 4 outright, regardless of offset.
let merged = a.meet(&b);
assert_eq!(merged.elements(), &[Clock::new(4, 900)]);
```

Use `Lexicographic` whenever a higher-level version/epoch makes the lower-level
counter meaningless once it ticks.

---

## 4. A cluster that rescales at runtime

Fixed-arity tuples cannot model a topology that changes shape — you cannot add a
shard without recompiling. `MapLattice<K, V>` keys progress by a dynamic identifier:
each shard appears in the map the moment it first reports.

- **`meet`** = key **intersection** + value-meet → "progress every shard agrees on"
- **`join`** = key **union** + value-join → "the furthest anyone has reached"

```rust
use antichain::{MapLattice, Lattice};

// Snapshot reported by node A: it has heard from shards 0 and 1.
let mut a = MapLattice::new();
a.insert(0u32, 100u64);
a.insert(1u32, 80u64);

// Node B has heard from shards 1 and 2 (shard 2 just came online).
let mut b = MapLattice::new();
b.insert(1u32, 95u64);
b.insert(2u32, 40u64);

// Conservative cluster-wide progress: only the shard both observed (1),
// taking the lower of the two values.
let safe = a.meet(&b);
assert_eq!(safe.get(&1), Some(&80));
assert_eq!(safe.len(), 1);

// Optimistic union: the furthest progress seen for every known shard.
let seen = a.join(&b);
assert_eq!(seen.get(&0), Some(&100));
assert_eq!(seen.get(&2), Some(&40));
assert_eq!(seen.len(), 3);
```

The empty map is the identity for `join` and the absorbing element for `meet`, so you
never need a magic "zero shard" sentinel.

---

## 5. Quorum / acknowledgement sets

When progress is gated on *which discrete members have responded* rather than on a
numeric clock, model the membership directly with `SetLattice<T>` (partial order =
subset inclusion).

- **`meet`** = **intersection** → "members everyone agrees have acknowledged"
- **`join`** = **union** → "every acknowledgement seen anywhere"

```rust
use antichain::{SetLattice, Lattice};

// Two coordinators each collected a partial set of ACKs.
let mut c1 = SetLattice::new();
c1.insert("node-a");
c1.insert("node-b");

let mut c2 = SetLattice::new();
c2.insert("node-b");
c2.insert("node-c");

// Universally-agreed acknowledgements (safe to act on): the intersection.
let agreed = c1.meet(&c2);
assert!(agreed.contains(&"node-b"));
assert_eq!(agreed.len(), 1);

// Full picture of who has acknowledged anywhere: the union.
let all = c1.join(&c2);
assert_eq!(all.len(), 3);
```

---

## 6. Tracking a lower bound instead of an upper bound

A `Frontier` merges by `meet`, which on `u64` computes `min`. That is exactly right
for "everything *below* here is complete." But sometimes you want the opposite —
"everyone has *at least* reached here," where the safe merge is `max`. Wrap the value
in `Max<T>` to invert the order:

```rust
use antichain::{Frontier, Max};

// "Each replica has applied AT LEAST this log index."
let r1 = Frontier::from_elem(Max(40u64));
let r2 = Frontier::from_elem(Max(55u64));

// meet now keeps the HIGHER value — the guaranteed-applied floor.
let guaranteed = r1.meet(&r2);
assert_eq!(guaranteed.elements()[0], Max(55u64));
```

Pair `Max<T>` with `Min<T>` in a tuple to carry a lower *and* upper bound through the
same frontier:

```rust
use antichain::{Frontier, Max, Min};

// (guaranteed-applied floor, lowest-known ceiling)
let a = Frontier::from_elem((Max(5u64), Min(20u64)));
let b = Frontier::from_elem((Max(8u64), Min(15u64)));

let merged = a.meet(&b);
assert_eq!(merged.elements()[0].0, Max(8u64));  // max(5, 8)
assert_eq!(merged.elements()[0].1, Min(15u64)); // min(20, 15)
```

---

## 7. Bounding antichain width with a finite range

If a dimension is confined to a known interval `[min, max]`, `Bounded<T>` clamps every
value into that range at construction. Because the range is finite, the number of
distinct incomparable values — and therefore the antichain width — is provably bounded.

```rust
use antichain::{Frontier, Bounded};

// Offsets that can never exceed [0, 1000].
let a = Frontier::from_elem(Bounded::new(300u64, 0, 1000));
let b = Frontier::from_elem(Bounded::new(700u64, 0, 1000));

let merged = a.meet(&b);
assert_eq!(*merged.elements()[0].value(), 300u64);

// Out-of-range inputs are clamped, never rejected.
let clamped = Bounded::new(5000u64, 0, 1000);
assert_eq!(*clamped.value(), 1000u64);
```

Keep all values in one antichain on the **same** `[min, max]` range; mixing ranges is
undefined behaviour by design (lattice ops use `self`'s bounds).

---

## 8. Signalling a permanently-closed stream

Real pipelines need explicit "this is over" and "this hasn't started" markers — and
hard-coding `u64::MAX` as EOF is a footgun. The lifted enums add those sentinels
structurally:

- **`WithTop<T>`** adds a `Top` above every value. `Top` **absorbs `join`** (a closed
  path stays closed) and is the **identity for `meet`** (a closed path imposes no
  lower bound on others).
- **`WithBottom<T>`** adds a `Bottom` below every value. `Bottom` **absorbs `meet`**
  ("no progress" pins the safe frontier down) and is the **identity for `join`**.

```rust
use antichain::{WithTop, Lattice};

let live = WithTop::Value(100u64);
let sealed: WithTop<u64> = WithTop::Top; // this source has hit EOF

// Joining with a sealed source: Top absorbs — progress jumps to "closed".
assert_eq!(live.join(&sealed), WithTop::Top);

// But meet with a sealed source imposes no constraint — the live value survives.
assert_eq!(live.meet(&sealed), WithTop::Value(100u64));
```

Compose them as `WithTop<WithBottom<T>>` to get a fully closed lattice
`Bottom < Value(t) < Top` without a single magic constant.

---

## 9. Composing everything

The types are not a fixed menu — they are building blocks. The partial order of a
composite is derived automatically from its parts, and the convergence guarantee is
preserved through every layer of nesting. A few combinations that show up in practice:

| Composite | Models |
|-----------|--------|
| `Frontier<ProductTimestamp<u64, u64>>` | independent (partition, offset) progress |
| `Frontier<Lexicographic<u64, u64>>` | (epoch, offset) where the epoch dominates |
| `MapLattice<ShardId, u64>` | per-shard watermarks with runtime rescaling |
| `MapLattice<ShardId, ProductTimestamp<u64, u64>>` | per-shard *two-dimensional* progress |
| `Frontier<(Max<u64>, Min<u64>)>` | a simultaneous lower and upper bound |
| `WithTop<WithBottom<u64>>` | a value that can be unstarted, live, or sealed |

```rust
use antichain::{MapLattice, ProductTimestamp, Lattice};

// Per-shard, two-dimensional (epoch, offset) progress that rescales at runtime.
type ShardClock = ProductTimestamp<u64, u64>;

let mut a: MapLattice<u32, ShardClock> = MapLattice::new();
a.insert(0, ShardClock::new(3, 100));
a.insert(1, ShardClock::new(3, 80));

let mut b: MapLattice<u32, ShardClock> = MapLattice::new();
b.insert(1, ShardClock::new(2, 200));

// Conservative merge: only shard 1 is shared; its meet is the component-wise min.
let safe = a.meet(&b);
assert_eq!(safe.get(&1), Some(&ShardClock::new(2, 80)));
```

---

## Worked end-to-end example

A complete three-layer progress protocol (Worker → Shard → Cluster) built entirely on
the public API lives in [`../examples/progress_protocol.rs`](../examples/progress_protocol.rs).
Run it with:

```sh
cargo run --example progress_protocol
```

It demonstrates that the core primitives are sufficient to express a real coordinator-free
protocol with **no** code reaching back into crate internals.

---

## Cheat sheet — `meet` vs `join`

| | `meet` (greatest lower bound) | `join` (least upper bound) |
|---|---|---|
| Intent | "the safe, conservative answer everyone agrees on" | "the furthest progress anyone has seen" |
| `u64` | `min` | `max` |
| `Max<u64>` | `max` (order inverted) | `min` |
| `ProductTimestamp` | component-wise `meet` | component-wise `join` |
| `MapLattice` | key **intersection** + value-meet | key **union** + value-join |
| `SetLattice` | set **intersection** | set **union** |
| `WithTop` | `Top` is identity | `Top` is absorbing |
| `WithBottom` | `Bottom` is absorbing | `Bottom` is identity |

The coordinator-free merge is almost always **`meet`**: it is the operation whose three
algebraic laws (commutativity, associativity, idempotence) make ordering-independent
convergence possible.
