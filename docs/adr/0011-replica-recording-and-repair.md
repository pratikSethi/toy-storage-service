# ADR 0011: Replica recording and repair

Status: accepted, partly built

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **R**: the number of copies of each chunk. R=3 means three copies. Only R=1 (one copy) is built.
> - **W**: how many of the R copies must be durable before a PUT is answered with 201. W=2 of R=3 means two of three.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **compare-and-swap (CAS)**: write only if the stored version is still the one you read, otherwise fail and retry.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## Context

With W=2 of R=3 ([ADR 0010](0010-replication-and-the-meaning-of-201.md)), the third write may finish after the ack, or never. The manifest has to say which replicas count, and repair has to change manifests without racing a concurrent overwrite.

## Decision

- **The manifest lists only acknowledged replicas.** Each chunk has a `node_ids` list with the nodes that confirmed the write before the ack. The manifest is the source of truth for reads.
- **Stragglers are not referenced.** A replica that finishes after the ack is not in the manifest, so it is an orphan. FrontdoorService does not cancel or delete it. GC removes it after the minimum age ([ADR 0013](0013-gc-ownership-per-cell.md)).
- **Repair edits the manifest with a compare-and-swap.** `PutManifest` takes the version the caller read. If the stored version differs, the write is rejected. Repair re-reads and retries or gives up, so it cannot bring back chunks of an object that was overwritten meanwhile.
- **Placement** uses rendezvous (highest random weight) hashing over healthy nodes, ranked per chunk id. Writes take the top R. Repair picks the next-ranked healthy node. Because reads follow the manifest, a change in the healthy set never makes existing data unfindable.

**Today:** built: rendezvous placement over healthy nodes, with one copy per chunk. The proto manifest has a `node_ids` list that holds one node. Not built: more than one replica, stragglers, the version check in `PutManifest` (reserved in the proto, not implemented) and the repair job.

## Consequences

- `PutManifest` gains a version check. This touches the ObjectIndexService API and the `ObjectIndex` trait.
- Repair is a scan of manifests for chunks with fewer than 3 live replicas, then copy, then CAS.
- Orphans from stragglers cost disk until the minimum age passes.
- Rendezvous hashing needs no ring state, only the node list and health.

## Alternatives considered

- **Manifest lists all attempted replicas.** Rejected: reads would try nodes that never got the bytes.
- **Cancel and delete the straggler in the PUT path.** Rejected: more work on the hot path, and a failed delete still leaves an orphan for GC.
- **Last-writer-wins repair.** Rejected: races with overwrite.
- **`hash(chunk_id) mod N`.** Rejected for replication: changing N moves almost everything.
