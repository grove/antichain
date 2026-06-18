# antichain-intervals

Disjoint-interval-set lattice for tracking **out-of-order progress with gaps**, built on
[`antichain`](https://crates.io/crates/antichain).

This is the companion crate implementing the Phase 7.4 `IntervalSetLattice` deferred from the
core crate (kept separate so the core stays lean and `no_std`-simple).

## When to use it

When progress arrives out of order and leaves holes — a backfill engine that has processed
blocks 150–200 while block 101 is still delayed. A single watermark cannot express "everything
*except* the gap"; an interval set can.

## The lattice

`IntervalSetLattice<T>` stores a canonical set of disjoint, non-adjacent, half-open intervals
`[start, end)`. The partial order is **set inclusion** over covered points:

- `meet` = **intersection** → the smallest commonly covered set (coordinator-free safe merge)
- `join` = **union with coalescing** → the largest covered set
- the empty set is the bottom element (identity for `join`, absorbing for `meet`)

Because it implements `antichain::Lattice`, it drops straight into a `Frontier` or a
`MapLattice` value.

```rust
use antichain_intervals::IntervalSetLattice;
use antichain::Lattice;

let mut a = IntervalSetLattice::new();
a.insert(100u64, 150);
a.insert(200, 250);

let b = IntervalSetLattice::from_interval(120u64, 210);

// Safe progress = what BOTH cover.
assert_eq!(a.meet(&b).intervals(), &[(120, 150), (200, 210)]);

// Optimistic coverage = union, touching ranges coalesced.
assert_eq!(a.join(&b).intervals(), &[(100, 250)]);
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `std`   | yes     | Link against `std`; disable for `no_std` + `alloc`. |
| `serde` | no      | Derive `Serialize` / `Deserialize`. |

## License

Apache-2.0
