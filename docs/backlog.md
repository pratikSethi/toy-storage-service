# Backlog

Features by milestone, taken from the [PRD](prd.md). Order inside a milestone is not fixed.

## v0.1

- Frontdoor: health, bucket CRUD, object PUT/GET/HEAD/DELETE, list.
- In-process SQLite index module and local-disk chunkstore module behind the real traits.
- OpenAPI from utoipa, Scalar UI, RFC 9457 errors, request ids, JSON logs.
- `ts2` CLI, docker compose.

## v0.2

- Placement rule (`hash(chunk_id) mod N`), 3 chunkstore nodes.
- Static API key auth.
- Config reload without redeploy.
- Per-request token from frontdoor to chunkstore ([ADR 0006](adr/0006-object-bytes-through-frontdoor.md)).
- Grafana dashboards.

## v0.3

- Multipart upload.
- Presigned URLs.
- Quotas and `GET /v1/quotas`.
- RocksDB index.

## v0.4

- Second cell, R=3 replication, failure drills.
- Registry extracted into a Go service (`registry/`) with gRPC.
- Jobs API (`202` plus `Location: /v1/jobs/{id}`).

## v0.5

- Erasure coding k=4 m=2.
- Scrub and repair.

## v0.6

- Extent-based chunk storage and a benchmark against file-per-chunk.

## v0.7

- Node join and leave, rebalance, rolling upgrade.
- Tiering to a cold tier (real S3).

## Unscheduled

- S3 wire-compat adapter (XML, SigV4).
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
