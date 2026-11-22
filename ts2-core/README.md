# ts2-core

Rust library crate. It is not a service. FrontdoorService links it in.

An object store needs a few rules that have nothing to do with HTTP: how to cut an object into chunks, how to hash them, which node a chunk goes to, and how to put a manifest and its chunks together without ever leaving a manifest that points at missing data. `ts2-core` holds those rules, so they can be tested alone and reused by other clients later.

## What it owns

- The manifest model (`Manifest`, `ChunkRef`) and limits.
- Chunking, hashing (SHA-256 for chunk checksums and the object ETag) and placement.
- Two async traits that mark the service boundaries:
  - `ObjectIndex`: buckets and manifests by (bucket, key), plus listing. Implemented by `SqliteObjectIndex` (in-process) and `remote::RemoteObjectIndex` (gRPC to ObjectIndexService).
  - `ChunkStore`: put, get (whole or ranged) and delete for chunks, each naming the node. Implemented by `LocalChunkStore`, which writes a temp file, fsyncs, renames, and fsyncs the parent directory, and by `remote::RemoteChunkStore` (gRPC to one ChunkStoreService per node, 1 MiB frames, 30 s put and 5 s other deadlines).
- `remote.rs` maps gRPC statuses to `Error` (`Unavailable`, `Timeout`, `DiskFull`, `ChunkNotFound`, ...), and `gc.rs` holds the orphan sweep: list a node's chunks first, then the index's refs, then delete unreferenced chunks older than the minimum age.
- `put_object`, `object_stream` and `delete_object`, which combine the two traits.

## Build and test

From the repo root:

```sh
cargo build -p ts2-core
cargo test -p ts2-core      # the remote tests run against in-process fake gRPC servers
```

## Design

[System overview](../docs/design/000-system-overview.md): section 5 (request paths and placement), section 6 (data model), section 7 (on-disk layout). See also [ADR 0003](../docs/adr/0003-random-chunk-ids-not-content-addressed.md) and [ADR 0006](../docs/adr/0006-object-bytes-through-frontdoor.md).
