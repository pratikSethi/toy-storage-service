# ADR 0004: Cells for the data plane, thin global layer

Status: accepted, partly built

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Context

One big shared data plane means any bug or overload can affect every tenant, and the system's size is unbounded, so it is never fully tested at the size it runs at. Whatever sits above the data plane is shared by everyone, so its failures hurt the most.

## Decision

The data plane is deployed as cells. A cell is a self-sufficient copy of the data plane (one ObjectIndexService plus N ChunkStoreService nodes) serving a fixed subset of buckets. Cells never talk to each other. Above the cells is a thin global layer: the FrontdoorService router and BucketRegistry (bucket-to-cell map, tenant and quota data). Per-cell control functions (node membership, node list used for placement, health, rebalancing) live inside the cell.

A tenant move copies manifests and chunks to another cell, flips the bucket in BucketRegistry, then garbage-collects the source.

**Today:** there is one cell (one ObjectIndexService and four ChunkStoreService nodes in compose). Not built: a second cell, the thin global layer, BucketRegistry and the bucket-to-cell map.

## Consequences

- A fault is contained to one cell.
- A cell has a fixed shape that can be tested. Capacity grows by adding cells.
- Rollouts go cell by cell.
- The global layer is a shared dependency and must stay small.
- A bucket cannot span cells, so bucket size is bounded by cell capacity.
- Tenant-wide quotas need either global counters (chosen for v0) or summing per-cell counters.

## Alternatives considered

- **One flat cluster.** Simpler, but unbounded blast radius and size.
- **Per-cell tenant and quota data.** Removes a global dependency from the write path, but makes tenant-wide quotas harder. Not chosen: quota counters are global in BucketRegistry for v0.
- **Cells that replicate to each other.** Rejected: breaks the isolation that is the point.
