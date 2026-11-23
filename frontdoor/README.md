# FrontdoorService

Rust (axum, tonic, tower, utoipa 5.x). The binary is `ts2-frontdoor`. It is the only part of TS2 that clients talk to.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **SQLite**: a small database that lives in one file.

When you upload an object, FrontdoorService checks the limits, cuts the bytes into chunks, writes them to chunk storage, and then writes the object's manifest (its size, ETag and ordered list of chunks) to the index. When you read, it looks up the manifest and streams the chunks back. By default the index is a SQLite file and chunk storage is a local directory, both provided by the [`ts2-core`](../ts2-core/README.md) library inside the same process. Set `TS2_INDEX_ADDR` and `TS2_CHUNK_NODES` and it talks gRPC to ObjectIndexService and ChunkStoreService instead (see below).

## What it owns

- The `/v1` REST API: buckets, objects (PUT, GET with ranges, HEAD, DELETE) and listing.
- Admission: size and key limits, errors as RFC 9457 problem details, request ids.
- The OpenAPI 3.1 spec (served at `/openapi.json`, with a Scalar UI at `/docs`) and its checked-in copy at `docs/api/openapi.yaml`, written by a test.
- Not built: API key auth, config reload, load shedding and a separate bucket service. Buckets are stored by the index.

## Build, test, run

From the repo root:

```sh
cargo build -p ts2-frontdoor
cargo test -p ts2-frontdoor      # also regenerates docs/api/openapi.yaml
make run                         # listens on 127.0.0.1:9000, data in ./data
```

Config is `config/frontdoor.yaml` (limits, listen address, data paths). Try the API with the [demo](../docs/demo.md).

## Remote mode

| Variable | Meaning |
|---|---|
| `TS2_INDEX_ADDR` | ObjectIndexService address, e.g. `127.0.0.1:50052`. |
| `TS2_CHUNK_NODES` | Comma-separated `node-id=address`, e.g. `node-1=127.0.0.1:50051`. The id is stored in manifests, so do not rename it. |
| `TS2_GC_MIN_AGE_SECS` | Orphan GC never deletes chunks younger than this. Default 3600. Keep it well above the longest upload. |
| `TS2_GC_INTERVAL_SECS` | Time between GC sweeps. Default 600. |

Set both addresses or neither. Remote mode connects lazily, so frontdoor starts before the services are up; requests fail with 503 until they are. Upstream failures map to 503 (unavailable), 504 (deadline) and 507 (node out of space). A chunk that a manifest references but its node no longer has is lost data and returns 500. At startup frontdoor asks each node who it is and refuses to start if a node reports a different id than the config; a background check keeps unhealthy or full nodes out of placement. Calls to the services repeat once on UNAVAILABLE when that is safe (never PutManifest). `TS2_PUT_TIMEOUT_SECS` (default 900) bounds a whole PUT, and `TS2_GC_MIN_AGE_SECS` must be at least twice that. Overwrite and delete leave the old chunks to GC. A background task deletes chunks that no manifest references (see `ts2-core/src/gc.rs`). `make e2e-native` runs the smoke test this way against the locally built C++ binaries.

## Design

[System overview](../docs/design/000-system-overview.md), section 2 (layers) and section 5 (request paths). A dedicated FrontdoorService design doc (001) is not written yet.
