# ADR 0009: Bucket-to-cell map

Status: accepted, not built yet

## Context

FrontdoorService has to know which cell owns a bucket ([ADR 0008](0008-what-a-cell-is.md)). The map could live in FrontdoorService config, in the first cell's index, or in the Go BucketRegistryService planned for v0.4. CreateBucket and ListBuckets behave differently depending on the choice.

## Decision

Until the Go BucketRegistryService exists (v0.4), the map is static and lives in FrontdoorService config.

- **CreateBucket** picks the cell with a rule in config, for example a default cell or a prefix match on the bucket name. The result is recorded in that cell's index and is stable afterwards.
- **Bucket names are globally unique.** FrontdoorService checks every cell before creating. Two concurrent creates of the same name in different cells can both pass the check. We accept that race at toy scale. The registry fixes it later by serialising name claims.
- **ListBuckets** fans out to all cells and merges the results.

**Today:** nothing here exists. There is one cell, no map, no create-time cell choice, no cross-cell name check and no ListBuckets fan-out.

## Consequences

- No new service and no new store for the first cells.
- Adding a cell is a config change and a FrontdoorService reload.
- The duplicate-name race is real, though unlikely with one operator. It is documented rather than hidden.
- ListBuckets latency is the slowest cell. A down cell makes the list partial or failed, and the API must say which.

## Alternatives considered

- **Map in the first cell's index.** Rejected: it makes one cell special and brings back a single point of failure for every cell.
- **Build BucketRegistryService now.** Rejected: it is a whole service for a problem that does not exist at one or two cells.
