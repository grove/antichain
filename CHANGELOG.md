# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-06-18

### Added
- **Phase 10 — Onboarding & ecosystem reach.**
- **Narrative tutorial** ([`docs/tutorial.md`](docs/tutorial.md)) — *"From One Number to a
  Frontier"*: a step-by-step walkthrough from a naive coordinator to a coordinator-free
  `Frontier` merge, introducing `meet`, the antichain invariant, and product order only as
  the story demands them. Compiled as a doctest.
- **Runnable examples:**
  - `examples/watermark_gossip.rs` — N workers exchanging frontiers over a simulated
    lossy network; prints convergence to a single global watermark.
  - `examples/backfill_gaps.rs` — out-of-order block processing with `IntervalSetLattice`
    from the `antichain-intervals` companion crate; demonstrates how gaps block safe
    acknowledgement until every worker fills the hole.
- **Prior-art comparison** ([`docs/comparison.md`](docs/comparison.md)) — fair comparison
  to timely-dataflow/differential-dataflow and to CRDT libraries; states what each is
  better at without strawmen.
- `README.md` "Learn more" section updated with links to the tutorial and comparison doc.

### Changed
- `Cargo.toml` `categories` expanded to include `concurrency` and `no-std` for better
  crates.io discoverability.

---

### Added (Phase 9, carried from Unreleased)
- **Cookbook guide** ([`docs/cookbook.md`](docs/cookbook.md)) — a task-oriented
  "which type for which problem" guide with a decision table and worked recipes for
  every public type. Its code blocks are compiled and run as doctests.
- `CHANGELOG.md`.
- **Phase 9 — `antichain-intervals` companion crate.** Ships the Phase 7.4
  `IntervalSetLattice<T>`: a disjoint-interval-set lattice (meet = intersection,
  join = coalescing union) for tracking out-of-order progress with gaps. Implements
  `antichain::Lattice`, supports `no_std` + `serde`, and is property-tested against a
  brute-force point-set oracle and the universal consistency law.
- **Universal consistency-law property tests** — a new module verifies the
  biconditional `a ≤ b ⟺ meet(a,b)==a ⟺ join(a,b)==b` in *both* directions for every
  lattice type (scalars, `ProductTimestamp`, `Lexicographic`, `Max`, `Min`, `Bounded`,
  `WithTop`, `WithBottom`, `MapLattice`, `SetLattice`, and nested compositions).
- **`forbid(unsafe_code)`** and **`deny(missing_docs)`** on both crates.
- **MSRV policy** (`rust-version = "1.85"`) and CI jobs for `no_std` builds, MSRV
  checking, and `cargo-semver-checks`.

### Changed
- **Performance: inline antichain storage.** `Antichain<T>` now stores width-0 and
  width-1 sets inline with **zero heap allocation**; only genuinely partially-ordered
  antichains of width ≥ 2 spill to a `Vec`. For totally-ordered timestamps
  (`Frontier<u64>`) the antichain always stays at width 1 and never allocates. The
  serde wire format is unchanged (`{ "elements": [...] }`).

### Fixed
- **`serde` feature was previously uncompilable.** The feature now enables serde's
  `alloc` collection impls and adds the `Ord` deserialize bounds that `MapLattice` /
  `SetLattice` require, so `--features serde` builds and round-trips. Locked in by new
  round-trip tests.

## [0.2.0] — 2026-06-18

### Added
- **Phase 5 — Formal specification.** A Fizzbee model-checking spec
  ([`specs/frontier_convergence.fizz`](specs/frontier_convergence.fizz)) that
  mechanically verifies the convergence theorem across every possible interleaving of
  update deliveries. Complementary `prop_tests_phase5` Rust property tests run 10 000
  random cases for `Frontier<u64>` and `Frontier<ProductTimestamp<u64, u64>>`.
- **Phase 6 — Extended composition patterns.** `Max<T>` (order-inverting wrapper so
  `meet` computes `max`), `Min<T>` (transparent complement for composite bounds), and
  `Bounded<T>` (finite `[min, max]` range with provable antichain-width bounds).
- **Phase 7 — Advanced structural & dynamic lattices.** `WithTop<T>` / `WithBottom<T>`
  (lifted sentinel enums), `MapLattice<K, V>` (point-wise lattice over `BTreeMap` for
  runtime-rescaling topologies), and `SetLattice<T>` (powerset / subset-inclusion
  lattice).
- **Phase 8 — Performance & real-world validation.** Documented empirical width bounds
  for `meet` (no compaction needed up to practical widths), and an end-to-end downstream
  adapter example ([`examples/progress_protocol.rs`](examples/progress_protocol.rs))
  proving the core primitives are sufficient for a real three-layer protocol.

### Changed
- **Phase 8.3 design-debt resolution:** `Bounded<T>` relaxed its bound from `T: Ord`
  to `T: PartialOrd`, so `Bounded<ProductTimestamp<…>>` now composes.

## [0.1.2] — 2026

### Added
- **Phase 4 — Hardening.** Criterion benchmarks ([`benches/frontier.rs`](benches/frontier.rs)),
  a `cargo-fuzz` target, `#![no_std]` compatibility behind the default `std` feature,
  feature-gated `serde` impls, and full rustdoc with inline law explanations.

## [0.1.1]

### Fixed
- Corrected documentation links.
- License updated to Apache-2.0.

## [0.1.0]

### Added
- **Phase 0–3.** Initial release: the `Lattice` trait, `Antichain<T>`, `Frontier<T>`,
  `ProductTimestamp<T1, T2>`, and `Lexicographic<A, B>`, with all algebraic laws
  (commutativity, associativity, idempotence, absorption, and the `PartialOrd`
  consistency law) verified by `proptest`.

[Unreleased]: https://github.com/trickle-labs/antichain/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/trickle-labs/antichain/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/trickle-labs/antichain/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/trickle-labs/antichain/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/trickle-labs/antichain/releases/tag/v0.1.0
