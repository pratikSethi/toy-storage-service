# ADR 0010: Replication R and W, and what 201 means

Status: accepted

## Context

With replication, a PUT can be acknowledged before every replica is written. We need to say what a 201 promises, and how many nodes a cell needs for that to hold when a node is down.

## Decision

Each cell has 4 ChunkStoreService nodes. Each chunk is written to R=3 of them, and a PUT succeeds once W=2 writes are durable.

A 201 means:

- the bytes are durable on at least 2 nodes in different failure domains (each node verified the sha256 and fsynced before replying), and
- the manifest is committed in the cell's index.

An acknowledged object can be under-replicated (2 copies, not 3) until repair catches up. The durability model and the SLO assume this. A repair job is required before W=2 ships, and until then the cell runs W=R=3.

**Why 4 nodes.** With 3 nodes and R=3, one node down means a PUT has nowhere to put its third replica, so writes stop. With 4, placement can pick any 3 healthy nodes and one node down does not stop writes. Four nodes also let us see placement: different chunks land on different subsets, which a 3-of-3 layout hides.

## Consequences

- Lower PUT latency and better PUT availability than W=3, at the price of a window with 2 copies.
- Losing 2 nodes inside that window can lose an acknowledged object. Repair time is the length of the window.
- Repair is a hard dependency of W=2 ([ADR 0011](0011-replica-recording-and-repair.md)).
- GET reads from any node listed in the manifest and falls through to the next on failure.

## Alternatives considered

- **W=R=3.** Simplest and no under-replication, but one node down fails every write on a 3-node cell, and the slowest node sets PUT latency. Kept as the interim setting.
- **R=2.** Rejected: one node loss leaves a single copy.
- **Quorum with R=3 nodes only.** Rejected for the availability reason above.
