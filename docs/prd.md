# TS2 Product Requirements

## Problem

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

- FR1. Create, list, get and delete buckets.
- FR2. PUT, GET, HEAD and DELETE objects. PUT returns an ETag.
- FR3. Range GET returns only the requested bytes and touches only the needed chunks.
- FR4. List objects in a bucket with `prefix`, `start_after` and `max_keys`. The response has `is_truncated` and a `next` marker. `max_keys` is capped at 1000 and keys are ordered bytewise.
- FR5. Objects under `inline_threshold_bytes` are stored inline in the manifest. Larger objects are split into fixed-size chunks.
- FR6. Every limit is enforced at admission with a specific error (413 for oversize objects, 400 for bad keys, 409 when deleting a non-empty bucket). Keys may contain slashes. Empty keys and `..` segments are rejected.
- FR7. Per-tenant quotas (buckets per tenant, bytes per bucket, requests/s) with defaults and overrides, readable at `GET /v1/quotas` (v0.3).
- FR8. Multipart upload (v0.3).
- FR9. Static API key authentication (v0.2). From v0.2 FrontdoorService also mints a per-request token that ChunkStoreService requires.
- FR10. A CLI, `ts2`, covering every API operation. It is written in Python (typer and httpx, generated client) and installed with `uv tool install`. A Rust rewrite sharing types with FrontdoorService is parked until distribution or multipart throughput needs it (see [ADR 0002](adr/0002-rust-frontdoor-go-bucket-registry-cpp-data-plane.md)).
- FR11. Limits and feature flags change by editing a config file, without redeploying.
- FR12. Presigned URLs (v0.3): a signed URL that FrontdoorService accepts. Bytes still flow through FrontdoorService ([ADR 0006](adr/0006-object-bytes-through-frontdoor.md)).
- FR13. Overwrite is last writer wins. `If-None-Match: *` on PUT makes it create-only (412 if the key exists).
- FR14. Integrity: the ETag is the hex SHA-256 of the object. From v0.2 a client may send `x-ts2-checksum-sha256` and a mismatch returns 400.
- FR15. GET and HEAD return `ETag`, `Content-Length`, `Content-Type` and `Accept-Ranges`. Range GET supports `a-b`, `a-` and `-n`. Unsatisfiable or multiple ranges return 416.

## Non-functional requirements

| Area | Requirement |
|---|---|
| Durability | Goal: no acknowledged object is lost. Not delivered in v0. R=3 replication in v0.4, erasure coding k=4 m=2 in v0.5. |
| Index loss | The single ObjectIndexService is the point of loss from v0.1 to v0.4. Replication and erasure coding protect chunks, not manifests, so losing the index loses every object in the cell. Mitigation: periodic index backups to a separate volume and a restore drill. |
| Full disk | A ChunkStoreService node stops accepting writes at a usage threshold (default 95%) and FrontdoorService returns 507 or tries another node. |
| Shutdown and recovery | Graceful drain on SIGTERM. ChunkStoreService clears `tmp/` at startup. |
| Slow clients and TLS | Header, idle and minimum-rate timeouts at FrontdoorService. TLS terminates at the load balancer. |
| Availability | SLO 99.9% per cell, measured at FrontdoorService as non-5xx responses. |
| Latency | p99 PUT under 500 ms for 10 MiB objects, measured locally (single machine, compose). |
| Consistency | Read-after-write for a key once PUT returns 201. A manifest never points at a missing chunk. |
| Overload | FrontdoorService sheds load with 429 or 503 plus `Retry-After`. It does not queue requests. |
| Portability | Services build and run natively on macOS and Linux. Container images are linux/arm64, so compose on a Mac runs Linux in a VM and the macOS fsync paths only run natively. |
| Operability | All services emit metrics and logs through OpenTelemetry and log `x-ts2-request-id`. Traces come later. |

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
- Code generated: FrontdoorService builds the OpenAPI 3.1 spec with utoipa 5.x from annotated axum handlers, serves it at `/openapi.json` and serves a Scalar UI at `/docs`. A test writes it to `docs/api/openapi.yaml` so changes show in review diffs. The Python CLI client is generated from that file.
- Errors use RFC 9457 Problem Details (`application/problem+json`), with `type` URLs into the docs site and a `request_id` extension.
- Every response carries `Server: ts2-frontdoor/<version>` and `x-ts2-request-id`.
- Every API is idempotent or carries an idempotency key. Long-running operations return 202 with `Location: /v1/jobs/{id}` (from v0.4).
- Endpoints: bucket CRUD, object PUT/GET/HEAD/DELETE, list objects (GET with query parameters; the IETF HTTP QUERY method is parked until it is an RFC), range GET.
- No auth and no rate limits in v0.1. A static API key header arrives in v0.2.

## Observability requirements

- OpenTelemetry SDK in every service.
- OTel Collector forwards metrics and logs over OTLP/HTTP (the `otlphttp` exporter) to VictoriaMetrics and VictoriaLogs. Grafana dashboards are provisioned from the repo. Traces come later (Tempo or VictoriaTraces).
- Alarms as listed in the [system overview](design/000-system-overview.md#9-observability).
- Every request is followable by `x-ts2-request-id` across FrontdoorService, ObjectIndexService and ChunkStoreService nodes, through logs and gRPC metadata. Distributed traces come later.

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
| v0.2 | C++ ChunkStoreService and ObjectIndexService replace the in-process modules. Placement rule, 3 nodes, API key auth and chunk token, limits reload, dashboards for the data plane. |
| v0.3 | Multipart upload, presigned URLs, quotas, RocksDB index. |
| v0.4 | Two cells in compose, R=3 replication, failure drills, BucketRegistry extracted into the Go BucketRegistryService, jobs API, index backups automated. |
| v0.5 | Erasure coding k=4 m=2, scrub and repair. |
| v0.6 | Extent-based ChunkStoreService and a benchmark against the file-per-chunk store. |
| v0.7 | Node join and leave, rebalance, rolling upgrade, tiering to real S3 as a cold tier. |
