# Build plan

Commit by commit. Each commit leaves a runnable system. Done steps carry their commit hash.

1. **Docs.** PRD, system overview, ADRs, backlog, this plan. Done, `f332510`.
2. **Scaffold.** `Makefile`, Cargo workspace (`frontdoor`, `ts2-core`), multi-stage `Dockerfile` that works with docker and podman, compose with FrontdoorService only, `mkdocs.yml` with the Material theme. Done, `57cc01a`.
3. **`ts2-core`.** Manifest model, the async `ObjectIndex` and `ChunkStore` traits, the SQLite and local-disk implementations (tmp, write, fsync, rename, fsync parent dir, `tmp/` cleared at startup), put and get with whole-chunk reads and checksum verification. Done, `5a08d40`.
4. **FrontdoorService v0.1.** Health, bucket CRUD, object PUT/GET/HEAD/DELETE/list, range GET, streaming `max_object_bytes` with axum's default body limit disabled, utoipa 5.x OpenAPI 3.1 and Scalar, RFC 9457 errors, request ids, JSON logs, compose healthcheck and data volume, curl demo. Done, `3f0efb0`.
5. **Python CLI and e2e smoke.** `cli/` with typer and httpx (generated client for JSON, plain `httpx` for object bytes), `tests/e2e/` with a smoke test through the CLI against `make run`. Needs the stable `docs/api/openapi.yaml` from step 4.
6. **Observability profile.** OTel Collector (`otlphttp` exporters), VictoriaMetrics, VictoriaLogs, Grafana with one FrontdoorService dashboard, behind `--profile obs`. Also an ObjectIndex backup job for the SQLite file.
7. **Proto and C++ scaffold.** `proto/ts2/v1` for the ChunkStoreService and ObjectIndexService calls, a CMake and vcpkg setup with Catch2, a Docker stage for C++, and CI that builds and tests every component. The Rust traits are already async and chunk-granular, so FrontdoorService changes only its client, not its logic.
8. **ChunkStoreService.** C++ with gRPC (`chunk-store/`), `put`, `get(range)`, `delete`, `list`. FrontdoorService switches from `LocalChunkStore` to it. Adds the per-request token and the full-disk threshold.
9. **ObjectIndexService.** C++ with SQLite (`object-index/`), manifests, listing, orphan GC (diff-based, minimum chunk age). FrontdoorService switches from `SqliteObjectIndex`. Its schema is written separately from the Rust one, which stays as the single-binary stand-in.
10. **Placement over 3 nodes (v0.2).** `hash(chunk_id) mod N` in `ts2-core`, three ChunkStoreService nodes in compose, API key auth.
11. **Second cell and replication (v0.4).** Two cells in compose, R=3, the bucket-to-cell map, failure drills.

Docs are served locally with `mkdocs serve` and published to GitHub Pages once the repo is public.
