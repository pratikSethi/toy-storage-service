# Build plan

Commit by commit. Each commit leaves a runnable system. Done steps carry their commit hash.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **R**: the number of copies of each chunk. R=3 means three copies. Only R=1 (one copy) is built.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **OTel** (OpenTelemetry): a standard way to emit metrics, logs and traces.
> - **fsync**: a system call that forces written data onto the disk before it returns, so a crash cannot lose it.
> - **SQLite**: a small database that lives in one file.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

1. **Docs.** PRD, system overview, ADRs, backlog, this plan. Done, `f332510`.
2. **Scaffold.** `Makefile`, Cargo workspace (`frontdoor`, `ts2-core`), multi-stage `Dockerfile` that works with docker and podman, compose with FrontdoorService only, `mkdocs.yml` with the Material theme. Done, `57cc01a`.
3. **`ts2-core`.** Manifest model, the async `ObjectIndex` and `ChunkStore` traits, the SQLite and local-disk implementations (tmp, write, fsync, rename, fsync parent dir, `tmp/` cleared at startup), put and get with whole-chunk reads and checksum verification. Done, `5a08d40`.
4. **FrontdoorService v0.1.** Health, bucket CRUD, object PUT/GET/HEAD/DELETE/list, range GET, streaming `max_object_bytes` with axum's default body limit disabled, utoipa 5.x OpenAPI 3.1 and Scalar, RFC 9457 errors, request ids, JSON logs, compose healthcheck and data volume, curl demo. Done, `3f0efb0`.
5. **Python CLI and e2e smoke.** `cli/` with typer and httpx (plain `httpx` throughout), `tests/e2e/` with a smoke test through the CLI against a throwaway frontdoor (`make e2e`). Done.
6. **Observability profile.** OTel Collector (`otlphttp` exporters), VictoriaMetrics, VictoriaLogs, Grafana with one FrontdoorService dashboard, behind `--profile obs`. Done, `14b88b9`. The ObjectIndex backup job for the SQLite file is still open.
7. **Proto and C++ scaffold.** `proto/ts2/v1` for the ChunkStoreService and ObjectIndexService calls, a CMake and vcpkg setup with Catch2, a Docker stage for C++, and CI that builds and tests every component. The Rust traits are already async and chunk-granular, so FrontdoorService changes mostly its client. The trait gains a node id and a few error variants, see step 11. Done, `139acef`.
8. **Contracts and build plumbing.** Done, `83ad65a` and `d284062`. Renamed the duplicate `NodeInfo` messages, closed the contract gaps from [review 002](reviews/002-round3-review.md) (1 MiB frames, id format, checksum sidecar, error codes, paging limits, `ListChunkRefs`, `previous` on `PutManifestResponse`), shared `cmake/ts2_proto.cmake`, root `vcpkg.json` with a pinned baseline, asan preset, `.dockerignore`, CI cache and concurrency fixes, and the `ts2-proto` Rust crate. This came first because steps 9 and 10 both build on `proto/`, `ci.yml` and the shared CMake file.
9. **ChunkStoreService.** C++ with gRPC (`chunk-store/`), `put`, `get(range)`, `delete`, `list`, sidecar checksums, graceful SIGTERM shutdown. It touches only `chunk-store/` and its own row in `docs/ports.md`. Done, `e7af98e`. The per-request token stays in v0.2. A put fails with `RESOURCE_EXHAUSTED` when free space is below the chunk size; a separate full-disk threshold is not built.
10. **ObjectIndexService.** C++ with SQLite (`object-index/`): manifests, listing, all four bucket RPCs, and `ListChunkRefs`. Done, `a39e072`. Its schema is written separately from the Rust one, which stays as the single-binary stand-in.
11. **FrontdoorService switches to gRPC.** Built after steps 9 and 10. tonic clients (from `ts2-proto`) in `ts2-core/src/remote.rs` behind the existing traits, tested against in-process fake gRPC servers. The `ChunkStore` trait takes a node id, `put_manifest` returns the replaced manifest so an overwrite deletes the old chunks at once, `Error` gains `Unavailable`, `Timeout`, `DiskFull` and `ChunkNotFound` (frontdoor answers 503, 504, 507, 404), and chunk ids must be exactly 32 lowercase hex. Frontdoor picks remote backends when `TS2_INDEX_ADDR` and `TS2_CHUNK_NODES` are set (default stays in-process) and runs orphan chunk cleanup (garbage collection) in the background in that mode only. Both C++ services join compose. Done, `ed59ae9`, `0a132b9` and `ecb82f7`. `make e2e-native` and `make e2e-compose` run the smoke test against the services. Not built: a Rust-side cap on the size of a cleanup sweep, and cleanup treats a node's chunk list as one in-memory list.
11a. **Hardening from [review 003](reviews/003-round4-review.md) findings 1 to 5.** Safe cleanup when `PutManifest` is ambiguous, no eager chunk deletes in remote mode, node id check and health-aware placement, PUT timeout tied to the cleanup minimum age, keepalive and retry once. Done, see [003-fixes](reviews/003-fixes.md).
11b. **Placement and Docker build speed.** Rendezvous placement in `ts2-core/src/placement.rs` (ordered list, R=1 for now), four ChunkStoreService nodes in compose and in `make e2e-native`, a vcpkg cache mount in both C++ Dockerfiles, and an e2e test that a 40 MiB object lands on more than one node. Done, see [004-placement](reviews/004-placement.md). `make e2e-native` and `make e2e-compose` pass.
12. **API key auth (v0.2).** Planned, not built.
13. **Second cell and replication (v0.4).** Planned, not built. Two cells in compose, R=3, the bucket-to-cell map, failure drills.

Docs are served locally with `mkdocs serve` and published to GitHub Pages once the repo is public.
