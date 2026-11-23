# Components

One section per component. Terms used here, in short: a **bucket** and **key** name an object. A **chunk** is a 4 MiB piece of an object. A **manifest** is the small record per object: size, ETag, and the list of chunks and which node holds each. The full definitions are in the [system overview](../design/000-system-overview.md).

> **Terms used here.** Defined once, so this page reads on its own.
>
> - **cell**: one ObjectIndexService plus its ChunkStoreService nodes. It is the unit a fault is meant to stay inside. One cell exists today.
> - **rendezvous hashing**: a way to pick nodes for a chunk. Every node gets a score for the chunk id and the highest scores win, so when a node leaves, only its chunks move.
> - **gRPC**: a way for services to call each other over the network (a remote procedure call, RPC), sending Protobuf messages over HTTP/2. The TS2 services talk to each other this way.
> - **OTel** (OpenTelemetry): a standard way to emit metrics, logs and traces.
> - **OTLP** (OpenTelemetry protocol): the wire format OTel data is sent in.
> - **SQLite**: a small database that lives in one file.
> - **ADR** (architecture decision record): a short note on one decision and why it was made.

## FrontdoorService (Rust)

The only thing clients talk to. It checks limits, cuts an upload into chunks, writes the chunks to ChunkStoreService nodes, then writes the manifest to ObjectIndexService. A read looks up the manifest and streams the chunks back in order. It also emits the `ts2_http_*` metrics and one JSON log line per request.

- Status: built. It runs alone with in-process backends, or against the C++ services. Not built: API key auth, config reload, load shedding.
- Design: [system overview, section 5](../design/000-system-overview.md), [ADR 0006](../adr/0006-object-bytes-through-frontdoor.md).
- Directory: `frontdoor/`.

## ts2-core (Rust library)

The shared logic that FrontdoorService uses: the data model, the `ObjectIndex` and `ChunkStore` traits, the in-process SQLite index and local-disk chunk store, and the gRPC clients for the C++ services. `make run` uses the in-process versions. Setting `TS2_INDEX_ADDR` and `TS2_CHUNK_NODES` switches FrontdoorService to the gRPC clients.

- Status: built, including the gRPC clients and rendezvous placement with one copy per chunk.
- Design: [system overview, sections 4 and 6](../design/000-system-overview.md).
- Directory: `ts2-core/`.

## ObjectIndexService (C++)

Keeps the manifest for every bucket/key in a cell. It is the one place that knows which chunks make up an object, so losing it loses every object in the cell until backups or a replicated index exist.

- Status: built. SQLite only, one process per cell, no backups yet.
- Design: [system overview, sections 3 and 6](../design/000-system-overview.md), [ADR 0004](../adr/0004-cells-for-the-data-plane-thin-global-layer.md).
- Directory: `object-index/`.

## ChunkStoreService (C++)

Stores chunks on a node's disk by chunk id and returns them on request, verifying the checksum. A cell has several nodes, and FrontdoorService picks the node for each chunk.

- Status: built. Four nodes in compose. The per-request write token check exists but is off by default.
- Design: [system overview, sections 5 and 7](../design/000-system-overview.md), [ADR 0003](../adr/0003-random-chunk-ids-not-content-addressed.md).
- Directory: `chunk-store/`.

## ts2 CLI (Python)

The command line client (`mb`, `rb`, `ls`, `cp`, `rm`, `head`). It speaks only the public REST API, so it knows about buckets and keys and nothing about chunks.

- Status: built. It calls the API with plain `httpx`, not a generated client.
- Docs: [CLI reference](../cli/index.md).
- Directory: `cli/`.

## Observability stack

FrontdoorService exports OTLP to an OpenTelemetry Collector, which feeds VictoriaMetrics (metrics) and VictoriaLogs (logs). Grafana shows the provisioned `frontdoor` dashboard. It runs with `make up-obs`. Thresholds and dashboards are described in [operations](operations.md).

- Status: built for FrontdoorService only. The C++ services write logs but export no metrics yet, and no alert rules exist. Both are planned for v0.2.
- Design: [system overview, section 9](../design/000-system-overview.md).
- Directory: `deploy/compose/`.

## Planned, not built

- **BucketRegistryService** (Go, v0.4): a separate service for buckets, tenants, quotas and the bucket-to-cell map. Today ObjectIndexService stores buckets, and `ts2-core`'s SQLite index does in single-process mode.
- **Second cell, replication and repair** (v0.4), **erasure coding** (v0.5), **extent-based storage** (v0.6).
- **Web UI** (TypeScript), **AWS CDK deploy**, **API key auth and quotas** (v0.2 and v0.3).

See the [backlog](../backlog.md) for the full list.
