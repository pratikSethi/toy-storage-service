# TS2

TS2 (Toy Storage Service) is an S3-like object store that you can run on a laptop. It is a learning build by Pratik Sethi, made to understand how an object store works inside.

**Status:** docs only. Nothing is implemented yet, and it is not durable in v0.

**Architecture:** three services, `frontdoor` (Rust) takes requests and moves object bytes, `index` (C++) maps keys to manifests, and `chunkstore` (C++) stores chunks. A `registry` (buckets, tenants, quotas) starts as a frontdoor module and becomes a Go service in v0.4. The `ts2` CLI is Python.

## What this is

A JSON REST object store with buckets, PUT, GET (with ranges), HEAD, DELETE and list. Large objects are split into 4 MiB chunks and spread over chunkstore nodes. The system is built from cells, each a complete copy of index plus chunkstores serving a fixed set of buckets. It has limits, error contracts, metrics, dashboards and failure drills, because that is where storage lessons come from.

## Why

To learn by building: admission, placement, chunk storage, metadata, cells and repair. Use S3 or MinIO if you need object storage for real work.

## What it is not

- Not S3 compatible. The API is JSON REST, not S3 XML with SigV4. An adapter may come later.
- Not durable in v0. Replication arrives in v0.4 and erasure coding in v0.5.
- Not multi-region, not hardened for multiple tenants, and not benchmarked against production systems.
- Not a place for data you cannot lose.

## FAQ

**Who is it for?** The builder first. The tenant types in the [PRD](docs/prd.md) (assets, media, analytics, ML datasets, backups) shape realistic requirements.

**What does v0.1 do?** One frontdoor with an in-process SQLite index and a local-disk chunkstore. PUT, GET, DELETE and list, a CLI and a docker compose setup.

**What does it promise?** A target of 99.9% availability per cell. Durability is a goal delivered in later milestones.

**Why so many languages?** Each layer gets the language that fits: Rust for frontdoor, Go for the registry, C++ for the data plane, Python for the CLI and tests. See [ADR 0002](docs/adr/0002-rust-frontdoor-go-registry-cpp-data-plane.md).

## Repo layout

Planned, see the [overview](docs/design/000-system-overview.md#10-deployment):

```
frontdoor/    Rust, S3-style front end
ts2-core/     Rust library: chunking, hashing, placement
registry/     Go, from v0.4
index/        C++
chunkstore/   C++
cli/          Python, the ts2 command
ui/           TypeScript, later
proto/        gRPC definitions
deploy/       compose, CDK
tests/e2e/    Python
docs/
```

## Reading the docs

Start at [docs/README.md](docs/README.md). Once the scaffold lands, `uv run mkdocs serve` serves them locally.

## License

To be decided.
