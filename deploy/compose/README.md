# deploy/compose

Docker compose setup for running TS2 in containers. It starts FrontdoorService (Rust) in remote mode plus ObjectIndexService and four ChunkStoreService nodes (`chunk-store-1` to `chunk-store-4`, node ids `node-1` to `node-4`, ports 50061 to 50064, all C++), each with its own named volume (`index-data`, `chunk-data-1` to `chunk-data-4`). FrontdoorService places each chunk on one of them by rendezvous hashing. The C++ services publish no host port; only frontdoor reaches them, over the compose network. The `ts2-data` volume is only used if you unset `TS2_INDEX_ADDR` and `TS2_CHUNK_NODES` on frontdoor. The observability profile is separate.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **fsync**: a system call that forces written data onto the disk before it returns, so a crash cannot lose it.

## What it owns

- `compose.yaml`: the three services, frontdoor's port (host `127.0.0.1:9001`, container 9000), the volumes, and healthchecks (`/healthz` for frontdoor, a TCP connect for the C++ services). Frontdoor waits for both to be healthy.
- GC timing for frontdoor: `TS2_GC_MIN_AGE_SECS` (default 3600) and `TS2_GC_INTERVAL_SECS` (default 600) from your shell.
- The images are linux/arm64. On a Mac that means Linux in a VM, so this setup does not exercise the macOS fsync paths. Use `make run` for those.

## Run

From the repo root:

```sh
make up       # build the image and start in the background
make down     # stop (the volumes are kept)
make e2e-compose   # run the CLI smoke test against 127.0.0.1:9001
```

Then use the API as in the [demo](../../docs/demo.md), with `127.0.0.1:9001` in place of `127.0.0.1:9000`. See the [port registry](../../docs/ports.md). The container reads `config/frontdoor.compose.yaml`.

## Design

[System overview](../../docs/design/000-system-overview.md), section 10 (deployment).
