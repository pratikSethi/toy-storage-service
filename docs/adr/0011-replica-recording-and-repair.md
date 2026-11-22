# ADR 0011: Replica recording and repair

Status: accepted

## Context

With W=2 of R=3 ([ADR 0010](0010-replication-and-the-meaning-of-201.md)), the third write may finish after the ack, or never. The manifest has to say which replicas count, and repair has to change manifests without racing a concurrent overwrite.

## Decision

- **The manifest lists only acknowledged replicas.** Each chunk has a `node_ids` list with the nodes that confirmed the write before the ack. The manifest is the source of truth for reads.
- **Stragglers are not referenced.** A replica that finishes after the ack is not in the manifest, so it is an orphan. FrontdoorService does not cancel or delete it. GC removes it after the minimum age ([ADR 0013](0013-gc-ownership-per-cell.md)).
- **Repair edits the manifest with a compare-and-swap.** `PutManifest` takes the version the caller read. If the stored version differs, the write is rejected. Repair re-reads and retries or gives up, so it cannot bring back chunks of an object that was overwritten meanwhile.
- **Placement** uses rendezvous (highest random weight) hashing over healthy nodes, ranked per chunk id. Writes take the top R. Repair picks the next-ranked healthy node. Because reads follow the manifest, a change in the healthy set never makes existing data unfindable.

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
