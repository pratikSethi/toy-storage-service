# TS2 Product Requirements

## Problem

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.
> - **inline object**: an object under 64 KiB. It is stored inside its manifest instead of as chunks.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **R**: the number of copies of each chunk. R=3 means three copies. Only R=1 (one copy) is built.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **OTel** (OpenTelemetry): a standard way to emit metrics, logs and traces.
> - **OTLP** (OpenTelemetry protocol): the wire format OTel data is sent in.
> - **fsync**: a system call that forces written data onto the disk before it returns, so a crash cannot lose it.
> - **SQLite**: a small database that lives in one file.
> - **p99**: the value that 99 of 100 requests beat. A p99 latency of 100 ms means 99 requests in 100 finish within 100 ms.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

Teams that need to store blobs (images, media, extracts, datasets, backups) rebuild the same pieces: size limits, error handling, placement, cleanup, dashboards. TS2 is one object store that provides these once. It is also a vehicle for learning storage internals, so each piece must be real enough to teach something.

## Users

Internal tenants, each a team that owns buckets:

| Tenant type | Typical objects | What they care about |
|---|---|---|
| Assets | images, static files, 10 KiB to a few MiB | small-object latency, GET throughput |
| Media | renders, video segments, up to the object limit | large PUT, range GET |
| Analytics | extracts, partitioned files | list by prefix, many keys |
| ML datasets | shards of tens to hundreds of MiB | sequential reads, range GET |
| Backups | large, write-once | durability, cheap storage |

The operator (Pratik) is also a user: dashboards, limits and failure drills matter as much as the API.

## Functional requirements

A requirement with a version in it is planned for that version and not built. The others are built.

- FR1. Create, list, get and delete buckets.
- FR2. PUT, GET, HEAD and DELETE objects. PUT returns an ETag.
- FR3. Range GET returns only the requested bytes and touches only the needed chunks.
- FR4. List objects in a bucket with `prefix`, `start_after` and `max_keys`. The response has `is_truncated` and a `next` marker. `max_keys` is capped at 1000 and keys are ordered bytewise.
- FR5. Objects under `inline_threshold_bytes` are stored inline in the manifest. Larger objects are split into fixed-size chunks.
- FR6. Every limit is enforced at admission with a specific error (413 for oversize objects, 400 for bad keys, 409 when deleting a non-empty bucket). Keys may contain slashes. Empty keys and `..` segments are rejected.
- FR7. Per-tenant quotas (buckets per tenant, bytes per bucket, requests/s) with defaults and overrides, readable at `GET /v1/quotas` (v0.3).
- FR8. Multipart upload (v0.3).
- FR9. Static API key authentication (v0.2). From v0.2 FrontdoorService also mints a per-request token that ChunkStoreService requires.
- FR10. A CLI, `ts2`, covering every API operation. It is written in Python (typer and httpx) and installed with `uv tool install`. A Rust rewrite sharing types with FrontdoorService is parked until distribution or multipart throughput needs it (see [ADR 0002](adr/0002-rust-frontdoor-go-bucket-registry-cpp-data-plane.md)).
- FR11. Limits and feature flags change by editing a config file, without redeploying (planned, v0.2). Today FrontdoorService reads its config at start.
- FR12. Presigned URLs (v0.3): a signed URL that FrontdoorService accepts. Bytes still flow through FrontdoorService ([ADR 0006](adr/0006-object-bytes-through-frontdoor.md)).
- FR13. Overwrite is last writer wins. Planned, not built: `If-None-Match: *` on PUT makes it create-only (412 if the key exists).
- FR14. Integrity: the ETag is the hex SHA-256 of the object. Planned (v0.2): a client may send `x-ts2-checksum-sha256` and a mismatch returns 400.
- FR15. GET and HEAD return `ETag`, `Content-Length`, `Content-Type` and `Accept-Ranges`. Range GET supports `a-b`, `a-` and `-n`. Unsatisfiable or multiple ranges return 416.

## Non-functional requirements

| Area | Requirement |
|---|---|
| Durability | Goal: no acknowledged object is lost. Not delivered today: each chunk has one copy. Planned: 3 copies of each chunk (R=3 replication) in v0.4, erasure coding k=4 m=2 in v0.5. |
| Index loss | The single ObjectIndexService is the point of loss from v0.1 to v0.4. Replication and erasure coding protect chunks, not manifests, so losing the index loses every object in the cell. Planned, not built: periodic index backups to a separate volume and a restore drill. |
| Full disk | Built: a node refuses a put when free space is below the chunk size, and FrontdoorService returns 507. Planned: a usage threshold (default 95%) and trying another node. |
| Shutdown and recovery | Graceful drain on SIGTERM. ChunkStoreService clears `tmp/` at startup. |
| Slow clients and TLS | Planned, not built: header, idle and minimum-rate timeouts at FrontdoorService, and TLS at a load balancer. Built: a PUT timeout. |
| Availability | Target: 99.9% per cell, measured at FrontdoorService as non-5xx responses. There is one cell today. |
| Latency | p99 PUT under 500 ms for 10 MiB objects, measured locally (single machine, compose). |
| Consistency | Read-after-write for a key once PUT returns 201. A manifest never points at a missing chunk. |
| Overload | Planned, not built: FrontdoorService sheds load with 429 or 503 plus `Retry-After`. |
| Portability | Services build and run natively on macOS and Linux. Container images are linux/arm64, so compose on a Mac runs Linux in a VM and the macOS fsync paths only run natively. |
| Operability | Built for FrontdoorService: metrics and logs through OpenTelemetry, with `x-ts2-request-id` in its logs. Planned: the same in the C++ services. Traces come later. |

## Limits

All limits live in the `limits` config block and are checked at admission. The values are small on purpose, so changing one live shows it is a knob.

| Limit | Default | Notes |
|---|---|---|
| `max_object_bytes` | 256 MiB | 413 when exceeded. Raising is safe. |
| `max_key_bytes` | 1024 | Lowering is unsafe: existing keys may exceed it. |
| `inline_threshold_bytes` | 64 KiB | Recorded per object, safe to change. |
| `chunk_bytes` | 4 MiB | Fixed per object and recorded in the manifest, safe to change. |

Anything recorded per object in the manifest can change later without migrating old objects. Anything that constrains data that already exists (lowering `max_key_bytes`) cannot.

## API principles

- JSON REST, versioned in the path (`/v1/...`). No S3 XML or SigV4.
- Code generated: FrontdoorService builds the OpenAPI 3.1 spec with utoipa 5.x from annotated axum handlers, serves it at `/openapi.json` and serves a Scalar UI at `/docs`. A test writes it to `docs/api/openapi.yaml` so changes show in review diffs.
- Errors use RFC 9457 Problem Details (`application/problem+json`), with `type` URLs into the docs site and a `request_id` extension.
- Every response carries `Server: ts2-frontdoor/<version>` and `x-ts2-request-id`.
- Every API is idempotent or carries an idempotency key. Planned (v0.4): long-running operations return 202 with `Location: /v1/jobs/{id}`.
- Endpoints: bucket CRUD, object PUT/GET/HEAD/DELETE, list objects (GET with query parameters; the IETF HTTP QUERY method is parked until it is an RFC), range GET.
- No auth and no rate limits today. A static API key header is planned for v0.2.

## Observability requirements

- OpenTelemetry SDK in every service. Built for FrontdoorService only.
- OTel Collector forwards metrics and logs over OTLP/HTTP (the `otlphttp` exporter) to VictoriaMetrics and VictoriaLogs. Grafana dashboards are provisioned from the repo. Traces come later (Tempo or VictoriaTraces).
- Alarms as listed in the [system overview](design/000-system-overview.md#9-observability). No alert rules are built yet.
- Planned: every request followable by `x-ts2-request-id` across FrontdoorService, ObjectIndexService and ChunkStoreService nodes, through logs and gRPC metadata. Today only FrontdoorService logs it. Distributed traces come later.

## Out of scope

- S3 wire compatibility (possible later adapter).
- Versioning, lifecycle rules, object lock, cross-region replication.
- Fine-grained IAM and per-object ACLs.
- Production-grade security hardening.
- A config service and a feature-flag service (see [ADR 0005](adr/0005-file-reload-config-before-config-service.md)).

## Milestones

| Version | Scope |
|---|---|
| v0.1 | Single-node happy path: FrontdoorService with an in-process SQLite `ObjectIndex` and a local-disk `ChunkStore` from `ts2-core`, PUT/GET/DELETE/list, CLI, compose, and an observability profile with one FrontdoorService dashboard. |
| v0.2 | C++ ChunkStoreService and ObjectIndexService replace the in-process modules. Placement rule, 4 nodes, API key auth and chunk token, limits reload, dashboards for the data plane. Partly built: both C++ services, four nodes and placement. Not built: API key auth, chunk token, limits reload, data plane dashboards. |
| v0.3 | Multipart upload, presigned URLs, quotas, RocksDB index. |
| v0.4 | Two cells in compose, R=3 replication, failure drills, BucketRegistry extracted into the Go BucketRegistryService, jobs API, index backups automated. |
| v0.5 | Erasure coding k=4 m=2, scrub and repair. |
| v0.6 | Extent-based ChunkStoreService and a benchmark against the file-per-chunk store. |
| v0.7 | Node join and leave, rebalance, rolling upgrade, tiering to real S3 as a cold tier. |
