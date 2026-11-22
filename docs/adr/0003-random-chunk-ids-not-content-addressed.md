# ADR 0003: Random chunk ids, not content-addressed

Status: accepted for v0

## Context

A chunk needs an id that the manifest records. The id can be random or derived from the chunk's content hash (content-addressed). Content addressing deduplicates identical chunks. It also means many manifests can share one chunk, so a chunk can be deleted only when nothing references it, which needs reference counting or a mark-and-sweep GC across all manifests.

## Decision

Chunk ids are random 128-bit values. The content hash is stored beside the id as the chunk's checksum, in the manifest and on ChunkStoreService, and is verified on read. FrontdoorService generates the id (in `ts2-core`) before the write. Each chunk belongs to exactly one manifest.

## Consequences

- Deleting an object makes its chunks orphans that can be removed without checking other manifests. Orphans are found by diff-based GC with a minimum chunk age, see the [overview](../design/000-system-overview.md#12-decisions-and-open-questions).
- No deduplication.
- The id carries no information, so placement, computed by `ts2-core` in FrontdoorService, uses `hash(chunk_id) mod N` in v0.2 and the manifest records the node. ObjectIndexService does not place chunks.
- Moving to content addressing later needs a manifest version bump and a GC redesign.

## Alternatives considered

- **Content-addressed ids.** Rejected for v0: dedup is not a goal, and refcount GC is a large piece of work.
- **Sequential ids from ObjectIndexService.** Rejected: makes it a bottleneck in the write path.
