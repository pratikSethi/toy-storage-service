# Backlog

Features by milestone, taken from the [PRD](prd.md). Order inside a milestone is not fixed. Items marked **Built** exist. All other items are planned and not built.

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **chunk**: a piece of an object, up to 4 MiB, stored on one ChunkStoreService node. A node is one process that owns one data directory.
> - **manifest**: the small record kept for each object. It holds the size and the ordered list of its chunks, with the node that holds each one.
> - **ETag**: a fingerprint of an object's bytes (the SHA-256 hash in hex). Two objects with the same ETag have the same content.
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **failure domain**: a set of things that fail together, such as one disk, one host or one rack.
> - **orphan chunk**: a chunk on a node that no manifest points to. A failed upload, an overwrite or a delete can leave one.
> - **garbage collection (GC)**: the background job that deletes orphan chunks once they are older than a minimum age.
> - **R**: the number of copies of each chunk. R=3 means three copies. Only R=1 (one copy) is built.
> - **W**: how many of the R copies must be durable before a PUT is answered with 201. W=2 of R=3 means two of three.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **compare-and-swap (CAS)**: write only if the stored version is still the one you read, otherwise fail and retry.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **OTel** (OpenTelemetry): a standard way to emit metrics, logs and traces.
> - **SQLite**: a small database that lives in one file.
> - **SLO** (service level objective): a target for a measurement over a period, for example 99.9% of requests succeed in a month.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## v0.1

- FrontdoorService: health, bucket CRUD, object PUT/GET/HEAD/DELETE, list.
- In-process SQLite `ObjectIndex` and local-disk `ChunkStore` in `ts2-core`, behind the real traits.
- OpenAPI from utoipa, Scalar UI, RFC 9457 errors, request ids, JSON logs.
- `ts2` CLI (Python), docker compose.
- Observability profile: OTel Collector, VictoriaMetrics, VictoriaLogs, Grafana with one FrontdoorService dashboard.
- ObjectIndex backup job (SQLite online backup). Not built.

## v0.2

- **Built:** C++ ChunkStoreService and ObjectIndexService replace the in-process modules.
- **Built:** placement in `ts2-core` by rendezvous hashing, four ChunkStoreService nodes. Single copy, no replication yet.
- **Built:** NodeInfo id check at FrontdoorService startup ([ADR 0012](adr/0012-node-ids-are-permanent.md)).
- **Built:** orphan chunk cleanup (garbage collection, GC: delete chunks no manifest points to, once older than a minimum age).
- End-to-end client checksum: the client sends a sha256 of the whole object (and of each part, once multipart exists) in `x-ts2-checksum-sha256`. FrontdoorService verifies it before committing the manifest and rejects a mismatch with 400. Today frontdoor computes the ETag from the bytes it received, which cannot catch corruption that happened before frontdoor.
- Static API key auth. Its own item, separate from placement and replication.
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

- Rendezvous placement over healthy nodes, 4 ChunkStoreService nodes per cell, R=3 ([ADR 0011](adr/0011-replica-recording-and-repair.md)). Manifest records `node_ids` per chunk.
- Replication with W=R=3 first. Then W=2 ([ADR 0010](adr/0010-replication-and-the-meaning-of-201.md)): 201 means 2 durable copies on different failure domains plus a committed manifest. Needs the repair job below.
- Repair job: finds under-replicated chunks, copies them, updates the manifest with a version compare-and-swap in `PutManifest`. Must exist before W=2 ships.
- Per-cell GC sweep in FrontdoorService, one sweeper per cell ([ADR 0013](adr/0013-gc-ownership-per-cell.md)).
- Second cell with a static bucket-to-cell map in FrontdoorService config, global name check across cells, ListBuckets fan-out ([ADR 0008](adr/0008-what-a-cell-is.md), [ADR 0009](adr/0009-bucket-to-cell-map.md)).
- Failure drills: one node down, a whole cell down.
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
