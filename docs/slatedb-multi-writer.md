# Multi-writer SlateDB with `antichain` and CRDTs

*How a coordinator-free progress primitive and conflict-free replicated data types can
turn SlateDB from a single-writer log-structured store into one that safely accepts writes
from many processes at once.*

> This is a design narrative, not a finished SlateDB RFC. It explains where `antichain`
> (the lattice/frontier crate in this repository) and CRDTs fit into the multi-writer
> problem, what they can do, and — just as importantly — what they cannot do on their own.
> For the plain-language background on the primitives themselves, start with the
> [FAQ](faq.md); for the type catalogue, see the [Cookbook](cookbook.md); for how this
> crate relates to CRDT libraries, see [Prior art & positioning](comparison.md).

---

## 1. The shape of the problem

SlateDB is an embedded log-structured merge-tree that writes all of its durable state —
the write-ahead log, the sorted-string tables, and the manifest that ties them together —
to object storage rather than to a local disk. That single design decision is what makes
SlateDB interesting, because object storage gives you bottomless capacity, cheap
durability, and effortless replication, all without running a storage cluster of your own.
But it also means that the one piece of shared mutable truth in the whole system, the
manifest, lives in a place that several processes can reach at the same time. Today SlateDB
leans on a strict *single-writer invariant*: exactly one process is allowed to advance the
manifest, and it protects that right with an epoch number and compare-and-swap writes on
object storage, so that a stale writer who wakes up after a network partition discovers it
has been fenced out and politely stops. This is simple, it is correct, and it is the right
default. The trouble is that "exactly one writer" is also a ceiling. A single writer caps
your ingest throughput at one machine's CPU and one machine's object-store bandwidth, it
makes that machine a single point of failure for the *write* path, and it forces awkward
failover dances whenever that machine dies or needs to be replaced.

Multi-writer SlateDB is the natural next step: let several processes accept `put`s and
`delete`s concurrently, each flushing its own SSTs to object storage, while the database as
a whole still presents a single, consistent, linearizable-looking key-value store to its
readers. The hard part is not the writing — many processes can happily PUT immutable SST
files into a bucket without ever colliding, because each file has a unique name. The hard
part is *agreement*: agreeing on what the current contents of the database are, agreeing on
the order in which concurrent writes happened, and agreeing on how far each writer has
durably progressed so that readers never see a torn or half-applied view. This is where the
two primitives in the title earn their keep. CRDTs give you a way to *merge the state*
several writers produce without a coordinator refereeing every change, and `antichain`
gives you a way to *merge the progress* several writers report so that everyone can compute
the same safe read boundary. They solve two different halves of the same coordination
problem, and they compose cleanly because they rest on the same piece of mathematics.

---

## 2. The one idea underneath both tools

Both CRDTs and `antichain` exploit a single, almost suspiciously simple insight from
lattice theory: *if your merge operation is commutative, associative, and idempotent, you
no longer need a coordinator.* Those three words are worth unpacking, because they are the
entire reason this approach works over the messy, lossy, out-of-order reality of object
storage and gossip. Commutative means merging A then B gives the same result as merging B
then A, so the order in which writers learn about each other's work does not matter.
Associative means you can merge in any grouping you like — fold three reports left-to-right
or right-to-left — and still land on the same answer, so you can batch and pipeline merges
freely. Idempotent means merging the same information twice changes nothing, so a writer can
re-read a manifest it has already seen, or a gossip message can be delivered three times,
and the result is identical. Put those together and you get *convergence*: no matter who
talks to whom, in what order, with what duplicates and delays, every process that has seen
the same set of facts computes the same state. There is no moment where the system is
"half-updated" and hands out a wrong answer, because every intermediate state is itself a
valid merge of some subset of the facts.

A CRDT is a data structure whose merge function is exactly such a lattice operation, usually
a *join* — the least upper bound, the "combine everything anyone has ever told me" move that
grows monotonically toward a more-complete state. `antichain`'s `Frontier` is the mirror
image: its headline operation is `meet`, the greatest lower bound, the "what is safely true
for *everyone*" move that finds the most conservative common ground. The
[comparison doc](comparison.md) puts it crisply — CRDTs model *what data is present* and
grow toward `⊤` (top, the fully-merged state), while `antichain` models *how far computation
has advanced* and reasons toward the conservative bound. SlateDB multi-writer needs both
directions at once: it needs to *accumulate* the union of everything every writer has
produced (a join, a CRDT's job), and it needs to *find the floor* of how far every writer
has durably committed so reads can be served safely (a meet, `antichain`'s job). Using one
consistent algebra for both halves means the correctness arguments rhyme, the same
property-based tests apply, and engineers only have to learn one mental model.

---

## 3. Where `antichain` fits: the global durable frontier

Start with the half that `antichain` was built for. In a single-writer SlateDB, "how far
have we durably committed?" is answered by one monotonically increasing sequence number —
the writer assigns sequence numbers to operations, flushes them in order, and a reader
knows that everything up to the last flushed sequence number is safe to read. The moment you
have several writers, that single number fractures into several, because writer one might be
durably flushed up to sequence 1,024 while writer two has only reached 987 and writer three,
mid-flush, is somewhere in between. The question a reader actually cares about — *what
sequence number has **every** writer durably persisted, such that I will not miss anything
if I read up to there?* — is precisely a `meet` over the writers' individual frontiers. This
is the textbook use of `Frontier<u64>`:

```rust
use antichain::Frontier;

// Each writer publishes its own durable-flush frontier (e.g. into the manifest,
// or gossiped peer-to-peer).
let writer_1 = Frontier::from_elem(1024u64); // flushed through seq 1024
let writer_2 = Frontier::from_elem(987u64);  // flushed through seq 987
let writer_3 = Frontier::from_elem(1003u64); // flushed through seq 1003

// The globally safe read boundary is the conservative minimum.
let safe = writer_1.meet(&writer_2).meet(&writer_3);
assert_eq!(safe, Frontier::from_elem(987u64)); // can't trust anything past 987
```

The beauty of expressing it this way, rather than as an ad-hoc `min()` scattered through the
read path, is that the algebra carries guarantees the bare minimum does not. Because `meet`
is commutative and associative, it does not matter whether a reader collects writer reports
from the manifest, from a gossip round, or from a mix of stale and fresh sources, nor in
what order it folds them — the safe boundary it computes is identical to what every other
reader computes from the same facts. Because `meet` is idempotent, a writer that re-reports
an unchanged frontier, or a gossip message that arrives twice, cannot corrupt the result.
And because the convergence behaviour of this exact operation has been
[formally model-checked](../specs/frontier_convergence.fizz) and property-tested over tens
of thousands of random cases in this crate, you are building the read-safety boundary of a
database on a primitive whose central claim has been mechanically verified rather than merely
argued on a whiteboard. SlateDB's own distributed-compaction design already reinvents a
weaker, hand-rolled version of this idea — workers heartbeat their progress, a coordinator
takes the conservative view of who is alive and how far they have gotten, and that view
drives garbage collection and manifest commits — so adopting `antichain` for the writer
frontier is less a new dependency than a principled replacement for a pattern the codebase
is already growing organically.

The single-number case is only the entry point, though, and the multi-writer problem rewards
richer shapes. A very natural multi-writer topology is *partitioned writers*, where each
writer owns a disjoint slice of the keyspace — writer one handles keys `a`–`h`, writer two
handles `i`–`p`, and so on. Now "global progress" is genuinely multi-dimensional: writer one
being far ahead on its partition tells you nothing about writer two's partition, and it would
be wrong to collapse them into a single scalar. This is exactly the situation
`Frontier<ProductTimestamp<PartitionId, SeqNo>>` models, where each partition advances
independently and the frontier honestly represents "partition one is done through here,
partition two through there" without pretending one is ahead of the other. If the set of
partitions itself changes at runtime — writers are added during a scale-out, or a partition
is split — then `MapLattice<PartitionId, Frontier<SeqNo>>` captures a keyed collection of
per-partition frontiers that merges key-by-key, so a rescaling event does not require any
coordinator to reissue everyone's view. The [Cookbook](cookbook.md) walks through each of
these shapes with compilable examples; the point for SlateDB is that the *same* `meet`-based
machinery scales from "one writer, one clock" all the way up to "a cluster of partitioned
writers that rescales while running" without changing the underlying correctness story.

---

## 4. Where CRDTs fit: merging the data itself

`antichain` answers "how far has everyone gotten?" but it deliberately says nothing about
"what did everyone actually write?" That second question — reconciling the concurrent *data*
produced by several writers into one coherent view of the database — is the province of
CRDTs, and it is the harder and more interesting half of multi-writer SlateDB. The reason it
is hard is that two writers can touch the same key at genuinely the same logical time, with
no happens-before relationship between them, and the system has to pick a winner (or merge
the two) in a way that *every* node agrees on, forever, regardless of the order in which they
learn about the two writes. The classic CRDT building block for a key-value store is the
*last-writer-wins register*: every value carries a logical timestamp, and when two versions
of the same key meet, the one with the higher timestamp wins, with ties broken
deterministically (say, by writer ID). Because "take the version with the larger timestamp"
is a join over a totally ordered timestamp lattice, it is automatically commutative,
associative, and idempotent — which means two SlateDB readers that have seen the same set of
SSTs will resolve every key identically, even if they read those SSTs in different orders.
SlateDB already assigns sequence numbers and already does LSM-style "newest version of a key
wins" resolution during reads and compaction; the multi-writer move is to make that
tie-breaking *globally deterministic* across writers by combining each writer's local
sequence number with a writer identifier into a timestamp that is total, monotonic, and
never collides. That is a small, well-understood change to the comparator, and it turns
SlateDB's existing merge-on-read into a bona fide CRDT merge.

The LWW register is the simplest case, and for many workloads it is all you need, but CRDTs
offer a whole vocabulary of richer mergeable types that map onto features SlateDB users
already ask for. A *grow-only* or *observed-remove set* is the natural model for secondary
indexes and tombstone tracking, where concurrent adds and removes from different writers must
reconcile without losing an add that raced a remove. A *PN-counter* (a pair of grow-only
counters, one for increments and one for decrements) gives you mergeable numeric aggregates,
which is exactly the shape of SlateDB's existing *merge operator* feature — a merge operator
is, in CRDT language, an application-supplied associative-commutative combine function, and
framing it that way tells you precisely which operators are safe under concurrent writes
(the ones whose combine is a semilattice) and which are not. The connection runs deep enough
that SlateDB's change-data-capture stream and its merge-operator API are already *almost*
CRDT interfaces wearing different names; making multi-writer correct is partly a matter of
recognising that and enforcing the algebraic laws those features quietly depend on. The
[comparison doc](comparison.md) makes the boundary explicit: a CRDT library models the *data*
your application stores, while `antichain` models the *time* at which processing has arrived,
and the recommended pattern is to **use a CRDT for the application data and `antichain` for
the progress fence that guards when that data is safe to read.** That single sentence is the
architectural thesis of this whole report.

---

## 5. Putting them together: a sketch of the protocol

Imagine three SlateDB writer processes pointed at the same bucket. Each one accepts `put`s
and `delete`s from its clients, batches them into a memtable, and periodically flushes that
memtable to object storage as an immutable SST under a globally unique name — no two writers
ever collide on a filename, so the *write* path needs no coordination whatsoever, which is
the first and biggest win object storage hands you for free. Each operation carries a CRDT
timestamp built from the writer's identity plus its local monotonic sequence number, so that
when these SSTs are later read or compacted together, the last-writer-wins (or set, or
counter) merge resolves every key deterministically no matter what order the SSTs are
visited in. Alongside each flush, every writer publishes its current *durable frontier* — the
sequence number through which its SSTs are guaranteed persisted — into a small per-writer
record that other processes can read or that writers gossip directly to one another. A reader
(or any process that needs a consistent snapshot) collects those per-writer frontiers and
takes their `meet`, yielding the single global sequence number below which the database is
fully durable and stable across *all* writers; it serves reads only up to that boundary, so
clients never glimpse a key that one writer has flushed but another has not yet caught up to.
The data merges by CRDT join, the read boundary merges by `antichain` meet, and neither
operation needs a central referee.

The reason this is correct under failure — and failure is the only thing that matters in a
distributed store — comes straight back to the three algebraic laws. If a writer crashes
mid-flush, its last *published* frontier simply stops advancing; the global `meet` holds the
read boundary back to that writer's last safe point, so readers degrade to "slightly stale"
rather than "wrong," and when the writer recovers or is replaced it resumes from exactly
where its durable frontier said it was. If the network partitions and writers gossip stale or
duplicated frontiers, idempotence and commutativity guarantee the surviving readers still
converge to a consistent (if conservative) boundary rather than flapping. If two writers
race on the same key, the CRDT join resolves it identically on every node that eventually
sees both SSTs, so there is no split-brain in the *data* even though there was genuine
concurrency in the *writes*. The system never has to stop the world, elect a leader for the
common case, or funnel every operation through one machine — the coordinator-free merge does
the agreeing, quietly, in the background, as a side effect of nodes exchanging the values
they already needed to exchange.

---

## 6. What these primitives do *not* solve

It would be dishonest to present `antichain` and CRDTs as a complete multi-writer recipe,
because the most dangerous failures in a multi-writer store live in the gaps these tools
deliberately leave open, and pretending otherwise is how distributed databases lose data.
The first and most important gap is *writer fencing*. CRDTs and frontiers assume that every
fact they merge is a fact someone genuinely intended to commit; they have no opinion about
whether a given process is *still allowed* to be writing at all. A zombie writer that was
declared dead, kept running through a long garbage-collection pause, and then woke up to flush
a stale SST is a correctness disaster that no amount of commutative merging will catch,
because the zombie's writes look algebraically identical to legitimate ones. This is exactly
why SlateDB's existing *epoch plus compare-and-swap* protocol must remain in force in a
multi-writer world: each writer needs a lease or epoch, every durable write must prove it
holds a current epoch, and a writer that has been fenced must be unable to land an SST that
anyone will ever read. `antichain` is a pure data type with no networking, no clocks, and no
distributed locking — by design, as the [FAQ](faq.md) stresses — so fencing is squarely the
application's job and not something the frontier can or should do for you.

The remaining gaps follow the same theme: these are *merge* primitives, not *protocol*
primitives. They do not assign sequence numbers, allocate writer identities, or decide who
owns which partition — those are membership and configuration concerns that need their own
small coordination mechanism (often the very object-store CAS that SlateDB already uses).
They do not, on their own, give you *strict* linearizability or cross-key transactions: a
last-writer-wins CRDT resolves each key independently and consistently, but if your
application needs an atomic multi-key invariant across concurrent writers, you need a
transaction protocol layered on top, and the conservative read frontier from `antichain` is a
useful building block for that but not a substitute for it. They do not bound the cost of
metadata: naive per-key version vectors or unbounded tombstone sets can balloon, which is why
SlateDB's bounded, downsampling *sequence tracker* and its garbage-collection boundaries
matter just as much as the merge algebra. And they do not absolve you of testing — although
here the news is genuinely good, because the same property-based and model-checking
techniques that already cover `antichain`'s convergence, and that the CRDT literature has
refined for years, apply directly to the multi-writer merge and give you a verification
strategy rather than a leap of faith.

---

## 7. Pros and cons

The headline advantage of building multi-writer SlateDB on `antichain` and CRDTs is that you
get *coordination-free correctness*, and almost every other benefit flows from that single
property. Because the merges are commutative, associative, and idempotent, there is no leader
to elect for the common path, no consensus round on the critical write path, and no single
machine that every operation must funnel through — which means write throughput scales with
the number of writers rather than being pinned to one node, the failure of any one writer
degrades the system to "slightly stale" rather than "down," and the whole thing runs happily
over the lossy, out-of-order, eventually-consistent surface that object storage actually
gives you. A second, quieter advantage is *intellectual economy*: both halves of the problem
rest on the same lattice algebra, so engineers learn one mental model, the correctness
arguments rhyme, and the very same property-based and model-checking techniques that already
guard `antichain`'s convergence apply to the data merge too. A third is *incrementality* —
SlateDB already assigns sequence numbers, already resolves keys newest-wins, and already has
a merge-operator API, so much of this is reframing and hardening machinery that exists rather
than bolting on a foreign subsystem. And because `antichain` is a tiny, dependency-light,
`no_std`-capable pure data type, adopting it adds essentially no operational surface area: it
is a library, not a service.

The costs are just as real and should be stated plainly. The most fundamental is that
coordination-free merging buys *eventual* and *conservative* consistency, not strict
serializability: the global read frontier is always the floor of every writer's progress, so
a single slow or partitioned writer holds the entire database's visible boundary back, and
last-writer-wins resolution silently discards the loser of a concurrent write rather than
detecting the conflict. Workloads that need atomic multi-key invariants or
read-your-own-writes across writers will not get them from these primitives alone. A second
cost is *metadata overhead and its management* — CRDT timestamps, per-writer frontier
records, and tombstones all consume space and must be bounded and garbage-collected, or they
will quietly grow without limit. A third is *semantic discipline*: the algebra only protects
you if every merge function genuinely obeys the lattice laws, so a merge operator that is
secretly order-dependent, or a timestamp scheme that can collide, reintroduces exactly the
split-brain the design was meant to prevent — and these bugs are subtle, because they only
surface under concurrency the tests may not exercise. Finally, the approach leans entirely on
an *external fencing mechanism* it does not provide; if SlateDB's epoch-and-CAS layer is
misconfigured, the whole edifice is unsound no matter how elegant the merges are.

---

## 8. Trade-offs

Most of the design decisions here are not "right versus wrong" but *dials* you tune for a
particular workload, and naming the dials is more useful than pretending there is one correct
setting. The first and most consequential is **consistency versus availability and latency**.
Taking the conservative `meet` of every writer's frontier is what lets readers proceed
without a coordinator, but it ties the global read boundary to the slowest participant, so a
writer that is merely slow — not dead — drags everyone's visible watermark backward. You can
tighten this by fencing and evicting laggards aggressively, which restores a fresher frontier
at the cost of churn and the risk of evicting a writer that was only briefly stalled; or you
can loosen it by tolerating staleness, which maximises availability but means readers see an
older view. There is no setting that gives you both a fresh boundary and tolerance of slow
writers, because that tension is inherent to conservative merging.

A second trade-off is **conflict resolution semantics versus simplicity**. Last-writer-wins is
trivial to implement, costs one timestamp per value, and is what most key-value workloads
expect — but it resolves conflicts by *throwing data away*, which is unacceptable for
counters, sets, or any value where both concurrent updates carry meaning. Richer CRDTs
(PN-counters, observed-remove sets) preserve that meaning and merge it correctly, but they
cost more metadata, more code, and more careful reasoning, and they constrain what the value
*is* allowed to be. The trade-off is between a cheap, lossy, universal default and a richer,
heavier, type-specific merge; a mature design will offer LWW as the default and richer CRDTs
opt-in per column family or per merge operator. A third dial is **writer topology**:
fully-overlapping writers (anyone may write any key) maximise flexibility and load-balancing
but force genuine concurrent-write conflict resolution on every key, whereas *partitioned*
writers (each owns a disjoint key range, modelled by `ProductTimestamp` or `MapLattice`)
sidestep most conflicts entirely at the cost of a partition-assignment mechanism and the risk
of hotspots when one partition runs hot. A fourth is **gossip versus manifest-mediated
progress sharing**: pushing frontiers peer-to-peer converges fast and avoids hammering the
object store, but adds a network path and its own failure modes, while piggybacking frontiers
on manifest or per-writer object-store records needs no new transport but reacts only as fast
as your polling interval and costs object-store operations. Each of these is a slider, and the
right position depends entirely on whether your workload prizes freshness, throughput,
simplicity, or operational minimalism.

---

## 9. Alternative approaches

It is worth being honest that coordination-free merging is *one* way to do multi-writer, not
the only way, and for some workloads a different point on the spectrum is the better
engineering choice. The most conservative alternative is to **keep the single-writer
invariant and scale around it** — shard the keyspace across many independent single-writer
SlateDB instances, each the sole writer of its shard, and route each key to its owning
instance. This keeps every instance trivially correct and linearizable within its shard,
needs none of the CRDT machinery, and is how a great many production systems actually achieve
write scale-out; its cost is that cross-shard operations and rebalancing become an
application concern, and you trade a hard data-merge problem for a routing-and-resharding
problem. A second alternative is **true consensus**: run a Raft or Paxos group, or lean on an
external coordination service, so that a quorum agrees on a total order of writes. This buys
you strict serializability and conflict *detection* rather than lossy resolution, which some
applications genuinely require — but it reintroduces a coordinator, a leader on the write
path, and the latency and availability cost of a quorum round, which is precisely the
overhead an object-storage-native database is usually trying to escape.

A third family of alternatives changes *what* coordinates rather than *whether* it does.
**Compare-and-swap on a single shared log or manifest** — letting all writers contend on
optimistic concurrency over a numbered sequence of object-store files, exactly the mechanism
SlateDB's distributed-compaction RFC already uses for claiming jobs — gives you a serialised
order without a long-lived leader, at the cost of contention that rises with writer count and
a retry storm under high concurrency. **Deterministic timestamp ordering via synchronised
clocks** (a Spanner-style TrueTime or a hybrid logical clock) can provide an external,
globally consistent order that makes last-writer-wins *meaningful* rather than arbitrary, but
it leans on clock infrastructure and bounded skew that not every deployment can guarantee. And
at the far end, a **dedicated sequencer** — one tiny, fast service whose only job is to hand
out a global order, with everything else scaling freely behind it — concentrates the
coordination into a single cheap component rather than eliminating it. The honest framing is
that `antichain` and CRDTs occupy a specific and attractive niche: maximal availability and
throughput, minimal operational surface, and *eventual* consistency with deterministic
conflict resolution. If your workload can live within those semantics — and a surprising
number can — it is hard to beat. If it cannot, the alternatives above trade some of that
availability or simplicity back for stronger ordering guarantees, and the right design is
often a *hybrid*: CRDT-and-frontier merging for the bulk of the data, with a narrow consensus
or CAS path reserved for the few operations that genuinely need a total order.

---

## 10. Summary: a clean division of labour

The cleanest way to hold all of this in your head is as a division of labour between three
layers, each doing the one thing it is good at. At the bottom, object storage plus SlateDB's
*epoch-and-CAS* fencing provides the foundation of safety: unique immutable files that never
collide, and a guarantee that fenced or zombie writers cannot land durable writes that anyone
will read. In the middle, *CRDTs* reconcile the data — they take the concurrent SSTs that
several legitimate writers produced and merge them, key by key, into one deterministic view
that every reader agrees on regardless of read order, turning SlateDB's existing
newest-version-wins and merge-operator machinery into a principled conflict-free merge. At the
top, `antichain`'s **`Frontier` and its `meet`** reconcile the *progress* — they take each
writer's durable boundary and fold them into the single conservative read frontier that tells
every reader exactly how far it can safely look, with commutative, associative, idempotent
guarantees that hold up under partitions, duplicates, and reordering. CRDTs answer *"what is
the data?"*, `antichain` answers *"how much of it is safe to read?"*, and the fencing layer
answers *"who was even allowed to write it?"* Multi-writer SlateDB needs all three; the
contribution of `antichain` and CRDTs is that they let the two genuinely hard agreement
problems — agreeing on data and agreeing on progress — be solved *without a coordinator*,
which is the whole reason object-storage-native databases are worth building in the first
place.

---

### Further reading in this repository

- [FAQ](faq.md) — plain-language introduction to frontiers, `meet`, and why coordinator-free
  merging works.
- [Cookbook](cookbook.md) — which `antichain` type to reach for, with compilable examples
  (`Frontier<u64>`, `ProductTimestamp`, `MapLattice`, and more).
- [Prior art & positioning](comparison.md) — the precise relationship between `antichain` and
  CRDT libraries, including the "CRDT for data, `antichain` for the progress fence" pattern.
- [`specs/frontier_convergence.fizz`](../specs/frontier_convergence.fizz) — the formal,
  model-checked convergence specification for the `meet` merge.
- [`examples/watermark_gossip.rs`](../examples/watermark_gossip.rs) — a runnable simulation of
  many workers converging on a global frontier over a lossy, coordinator-free network.
