# ADR 0008: What a cell is

Status: accepted

## Context

[ADR 0004](0004-cells-for-the-data-plane-thin-global-layer.md) chose cells for the data plane but left open what is inside one. Without that, nobody can say what a failure is allowed to take down, and the per-cell availability numbers in [service levels](../engineering/service-levels.md) have no meaning.

## Decision

A cell is one ObjectIndexService plus its own set of ChunkStoreService nodes. FrontdoorService is not part of a cell. It is the shared, thin layer above all cells and routes each request to a cell by bucket.

Every bucket belongs to exactly one cell. Its manifests live in that cell's index and its chunks live on that cell's nodes. A cell never reads another cell's index or nodes.

The failure domain this contains: a corrupt or lost index, a bad ObjectIndexService or ChunkStoreService deploy, or full nodes affect only the buckets in that cell. Buckets in other cells keep working.

Per-cell availability in [service levels](../engineering/service-levels.md) follows from this. A cell's availability is measured for the buckets it owns.

## Consequences

- FrontdoorService stays stateless about object data and carries the routing table (see [ADR 0009](0009-bucket-to-cell-map.md)). A FrontdoorService bug can still hit every cell. Cells do not protect against that.
- Adding capacity means adding a cell, not growing one.
- A bucket cannot span cells. Moving a bucket between cells is a copy job and is not planned.
- Cross-cell operations (ListBuckets, global name checks) have to fan out from FrontdoorService.

## Alternatives considered

- **Cell includes its own FrontdoorService.** Closer to the strictest cell designs, but it needs a router in front anyway and duplicates policy (auth, quota, admission) per cell. Rejected for a toy-scale system.
- **One shared index, cells only for nodes.** Rejected: the index is the single point of loss, so sharing it defeats the point of a cell.
