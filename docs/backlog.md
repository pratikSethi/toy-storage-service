# Backlog

Features by milestone, taken from the [PRD](prd.md). Order inside a milestone is not fixed.

## v0.1

- FrontdoorService: health, bucket CRUD, object PUT/GET/HEAD/DELETE, list.
- In-process SQLite `ObjectIndex` and local-disk `ChunkStore` in `ts2-core`, behind the real traits.
- OpenAPI from utoipa, Scalar UI, RFC 9457 errors, request ids, JSON logs.
- `ts2` CLI (Python), docker compose.
- Observability profile: OTel Collector, VictoriaMetrics, VictoriaLogs, Grafana with one FrontdoorService dashboard.
- ObjectIndex backup job (SQLite online backup).

## v0.2

- C++ ChunkStoreService and ObjectIndexService replace the in-process modules.
- Placement rule in `ts2-core` (`hash(chunk_id) mod N`), 3 ChunkStoreService nodes.
- Orphan chunk GC (diff-based, minimum chunk age).
- End-to-end client checksum: the client sends a sha256 of the whole object (and of each part, once multipart exists) in `x-ts2-checksum-sha256`. FrontdoorService verifies it before committing the manifest and rejects a mismatch with 400. Today frontdoor computes the ETag from the bytes it received, which cannot catch corruption that happened before frontdoor.
- Static API key auth.
- Config reload without redeploy.
- Per-request token from FrontdoorService to ChunkStoreService ([ADR 0006](adr/0006-object-bytes-through-frontdoor.md)).
- Grafana dashboards for ObjectIndexService and ChunkStoreService.

## v0.3

- Multipart upload: initiate, upload part, complete, abort and list parts, each part with its own checksum. Includes GC of abandoned uploads (parts never completed or aborted, removed after a minimum age).
- Resumable uploads with client-side chunking: the `ts2` CLI splits a file into parts, uploads each with its own checksum, and records the parts the server confirmed in a local state file. After an interruption it resumes from the first missing part. Depends on multipart upload.
- Placement inspection: an admin endpoint plus `ts2 stat ts2://bucket/key` that prints the manifest (inline or chunked, and per chunk the id, node, length, sha256 and on-disk path). For demos and debugging.
- Presigned URLs.
- Quotas and `GET /v1/quotas`.
- RocksDB-backed ObjectIndexService.

## v0.4

- Second cell, R=3 replication, failure drills.
- Replication ack policy: with R replicas per chunk, frontdoor returns 201 only after W replicas are durable (each node verified the sha256 and fsynced before replying) and the manifest is committed. Pick W (W=R=3, or W=2 with background repair) and document the trade-off between write latency and durability.
- BucketRegistry extracted into the Go BucketRegistryService (`bucket-registry/`) with gRPC.
- Automated index backups and a restore drill (the index is the single point of loss until a replicated index exists).
- Jobs API (`202` plus `Location: /v1/jobs/{id}`).

## v0.5

- Erasure coding k=4 m=2.
- Scrub and repair.

## v0.6

- Extent-based ChunkStoreService storage and a benchmark against file-per-chunk.

## v0.7

- Node join and leave, rebalance, rolling upgrade.
- Tiering to a cold tier (real S3).

## Unscheduled

- S3 wire-compat adapter (XML, SigV4).
- Sub-chunk checksums for partial range reads.
- `If-Match` and object versioning.
- Direct-to-cell client built on `ts2-core`.
- Erasure coding profile tuning (other k and m values).
- Vault for keys and chunkstore node certs.
- Kubernetes: k3d and Helm.
- AWS CDK deploy.
- HTTP QUERY method, after it becomes an RFC.
- Config service, OpenFeature and flagd.
- Temporal as the engine for jobs.
- Key and cert rotation jobs.
- GitHub Pages for docs, once the repo is public, with a link from pratiksethi.dev.
- Per-bucket region (and later availability zone) chosen at CreateBucket. Needs a location field in the API and BucketRegistry's bucket-to-cell map. Not in the API today.
- Ops Review Grafana dashboard: availability by region and cell with the SLO lines drawn on, built from the real `ts2_http_*` metrics. See [operations](engineering/operations.md).
