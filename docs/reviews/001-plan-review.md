# Review 001: TS2 plan and design docs

Read-only review of README, PRD, plan, backlog, overview and ADRs 0001-0006. Checked from knowledge, no web lookups. Paths are relative to the repo root.

## 1. Inconsistencies

- Who picks the node for a chunk? `docs/design/000-system-overview.md:116-117` has frontdoor ask the index to "place chunks". `:80` and `docs/adr/0006-...:16` put placement in `ts2-core` (frontdoor), and `:128` says `hash(chunk_id) mod N`, which needs no index. Pick one.
- GC has two mechanisms. `000:182` and `:179` return a chunk list on DELETE and the index GCs it. `000:302` says GC lists chunks on each node and diffs against manifests. Keep the diff-based one and drop the chunk list.
- Stale anchor: `docs/prd.md:74` links `#observability`, but the heading is `## 9. Observability` (`000:259`), so the anchor is `#9-observability`.
- Platform: `prd.md:44` says "runs on macOS and Linux". `000:282` says the build target is linux/arm64 only, and compose runs Linux containers. The macOS fsync paths (`000:238`) would never run.
- Traces: `prd.md:45` says all services emit traces. `prd.md:73` and `000:261` say traces come later. `prd.md:75` promises cross-service traceability, which needs at least request-id logs.
- Milestones vs plan: `prd.md:89` puts the CLI and compose in v0.1, but `docs/plan.md:11` puts the CLI at step 7. Plan step 4 (Grafana, `plan.md:8`) is PRD v0.2. Plan step 8 bundles v0.2 placement and v0.4 cells and replication.
- `docs/adr/0001-...:17` says "the spec cannot drift from the code". The same line admits it depends on handler annotations, which is drift. Reword.
- v0.2 chunkstore token (`adr/0006:18`, `backlog.md:17`) is missing from the overview and the PRD. Presigned URLs (`backlog.md:23`) have no PRD FR.
- Naming: old names are gone. Leftovers are `README.md:39` ("S3-style front end", but ADR 0001 says not S3) and `000:19` ("Planes", the term is not used elsewhere).

## 2. Wrong or doubtful premises

- PUT invariant (`000:103`) holds only if the data is durable first. The write path (`000:234`) omits the fsync of the parent directory after `rename`. Without it a crash can lose the chunk file while the manifest survives.
- Orphan GC races with PUT (`000:302`). A chunk written but not yet committed looks like an orphan. GC needs a minimum age (for example 24 h) or it breaks the invariant. Also unspecified: crash cleanup of `tmp/`.
- Range mapping (`000:151`) is fine if every chunk except the last is exactly `chunk_bytes`. But the checksum covers the whole chunk (`000:215`). A partial read cannot verify it (`000:164` shows "bytes + checksum"). Either read whole chunks and slice, or add sub-chunk checksums. Also unspecified: suffix and open-ended ranges, 416, multi-range.
- GET verify (`000:144`): after the first bytes are streamed, a later checksum failure can only abort the connection. State that.
- SQLite then RocksDB (`000:83`, `prd.md:91`): not wrong, but costly. RocksDB under vcpkg builds slowly, and the swap also changes the transaction and list code. The index is also one process with no replica. R=3 and EC (`prd.md:39`) protect chunks, not the manifest, so losing the index loses all objects. SQLite on macOS only uses `F_FULLFSYNC` if `PRAGMA fullfsync=ON` (default off).
- macOS fsync (`000:238`): correct that `fsync` does not flush the drive cache and `F_FULLFSYNC` does. `O_DIRECT` needs aligned buffers and fails on some filesystems (tmpfs, some overlayfs). Docker Desktop runs Linux in a VM, so the Mac demo never exercises `F_FULLFSYNC`.
- OTel to VictoriaMetrics/VictoriaLogs (`000:261`): both accept OTLP over HTTP only, not gRPC. The Collector needs the `otlphttp` exporter with per-signal paths. The Rust and C++ OTel metrics and logs SDKs are younger than Go's, so budget for that.
- utoipa 3.1 (`000:88`): utoipa 5 emits OpenAPI 3.1, utoipa 4 emits 3.0. Pin 5.x. `openapi-python-client` reads 3.1, but it buffers request bodies in memory, so large streaming uploads (`ts2 put`) will not stream. Plan raw `httpx` for object bytes.
- Axum defaults to a 2 MiB body limit. PUT must disable it and enforce `max_object_bytes` while streaming (`prd.md:53`).

## 3. Plan risks

- Step 3 (`plan.md:7`) is 5+ commits: bucket CRUD, object path, SQLite module, local chunk module, utoipa/Scalar, errors, request ids, logs. Split it.
- Step 2 mixes Cargo workspace, Dockerfile, compose and mkdocs. Move mkdocs out.
- No step for `proto/` or CI. Steps 5 and 6 each need the proto, a vcpkg setup and a Docker stage. Add a "proto + C++ scaffold" commit before 5.
- The in-process traits (`plan.md:7`) must be streaming and async from day one, shaped like the gRPC calls. Otherwise steps 5 and 6 rewrite frontdoor. The SQLite index is written twice (Rust, then C++) with two schemas. Share one `schema.sql` or accept the rewrite explicitly.
- The CLI at step 7 hands back to frontdoor: the generated client needs a stable `openapi.yaml`. Move the CLI and a smoke e2e right after step 3.
- Missing for a curl-able v0.1: key paths with slashes (`/objects/{*key}`, percent-decoding, `..` and empty keys), `Content-Length` vs chunked uploads, GET/HEAD headers (`ETag`, `Content-Length`, `Content-Type`, `Accept-Ranges`), 409 on deleting a non-empty bucket, bucket name rules, a data volume in compose, a healthcheck, `spawn_blocking` for rusqlite, and a curl script in the README.

## 4. Missing from the PRD

- Overwrite semantics and concurrency: last writer wins, `If-None-Match`/`If-Match`.
- List pagination contract: `is_truncated` and a `next` marker, `max_keys` cap, ordering.
- End-to-end integrity: client-supplied checksum header, and which hash is the ETag (md5 or sha256).
- Index durability and backup, since it is the single point of loss (v0.1-v0.4).
- Full-disk behavior: stop writes at a threshold and return 507 or 503 (`000:267` only alarms).
- Crash recovery and graceful shutdown: tmp cleanup, startup scan, drain.
- Timeouts and slow-client limits, and TLS (at least via the load balancer).
- Chunkstore authentication before v0.2: ports are open in compose.

## 5. Mermaid

Not rendered: `mmdc` is absent, and `npx` would have to download it. By eye:

- `000:30-49` flowchart: valid. `subgraph Thin global layer` (`:35`) has a spaced title with no id. Safer as `subgraph Global["Thin global layer"]`. Edges to `Cell1` and `Cell2` before they are declared are fine.
- Sequence diagrams (`000:106-195`): `par ... end` balanced, `-)` needs Mermaid 10+ (GitHub supports it). No stray `;` or `#`.

## Top 3 to fix before coding continues

1. Decide placement ownership and replace the GC design with diff-plus-minimum-age (`000:116`, `:179`, `:302`), including the directory fsync (`000:234`).
2. Resolve range reads vs whole-chunk checksums (`000:151`, `:215`), then lock the manifest schema.
3. Re-cut `docs/plan.md`: split step 3, add proto/C++ scaffold and CI, move the CLI and e2e up, and make the v0.1 traits streaming.
