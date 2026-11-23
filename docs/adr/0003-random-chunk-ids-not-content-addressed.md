# ADR 0003: Random chunk ids, not content-addressed

Status: accepted for v0, built

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Context

A chunk needs an id that the manifest records. The id can be random or derived from the chunk's content hash (content-addressed). Content addressing deduplicates identical chunks. It also means many manifests can share one chunk, so a chunk can be deleted only when nothing references it, which needs reference counting or a mark-and-sweep GC across all manifests.

## Decision

Chunk ids are random 128-bit values. The content hash is stored beside the id as the chunk's checksum, in the manifest and on ChunkStoreService, and is verified on read. FrontdoorService generates the id (in `ts2-core`) before the write. Each chunk belongs to exactly one manifest.

**Today:** built. Placement uses rendezvous hashing over healthy nodes (`ts2-core/src/placement.rs`), not `hash(chunk_id) mod N` as written in the consequences below.

## Consequences

- Deleting an object makes its chunks orphans that can be removed without checking other manifests. Orphans are found by diff-based GC with a minimum chunk age, see the [overview](../design/000-system-overview.md#12-decisions-and-open-questions).
- No deduplication.
- The id carries no information, so placement, computed by `ts2-core` in FrontdoorService, uses `hash(chunk_id) mod N` in v0.2 and the manifest records the node. ObjectIndexService does not place chunks.
- Moving to content addressing later needs a manifest version bump and a GC redesign.

## Alternatives considered

- **Content-addressed ids.** Rejected for v0: dedup is not a goal, and refcount GC is a large piece of work.
- **Sequential ids from ObjectIndexService.** Rejected: makes it a bottleneck in the write path.
